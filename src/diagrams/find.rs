//! Finding the program that draws a language's diagrams when the
//! configuration file names none and kiri has no renderer of its own for
//! it, such as `mmdc` in a build without merman.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use super::{BuiltIn, Command, Renderer};
use crate::config::Environment;

/// mermaid-cli's command line, which the configuration file can also give.
pub const MMDC: &[&str] = &[
    "mmdc",
    "-i",
    "{input}",
    "-o",
    "{output}",
    "-t",
    "{theme}",
    "-b",
    "transparent",
    "-c",
    "{config}",
    "-s",
    "{scale}",
];

/// The command line kiri looks for to draw `language` when nothing is
/// configured; its first part is the program.
pub fn default_command(language: &str) -> Option<&'static [&'static str]> {
    language.eq_ignore_ascii_case("mermaid").then_some(MMDC)
}

/// Homebrew's directories for programs, on macOS and on Linux.
const HOMEBREW: &[&str] = &["/opt/homebrew/bin", "/home/linuxbrew/.linuxbrew/bin"];

/// Where kiri looks for a program: the directories on `PATH`, then where
/// npm, nvm and Homebrew install programs that may not be on it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Search {
    /// `PATH`.
    pub path: Option<OsString>,
    pub home: Option<PathBuf>,
    /// `NVM_DIR`, nvm's directory; `~/.nvm` when unset.
    pub nvm_dir: Option<PathBuf>,
    /// npm's global prefix from `NPM_CONFIG_PREFIX`; otherwise `prefix`
    /// in `~/.npmrc` tells it.
    pub npm_prefix: Option<PathBuf>,
    /// Further directories, looked in last: Homebrew's.
    pub others: Vec<PathBuf>,
}

impl Search {
    /// Where to look in the environment `env`.
    pub fn new(env: &Environment) -> Self {
        let set = |value: &Option<OsString>| {
            value
                .as_ref()
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        Search {
            path: env.path.clone(),
            home: set(&env.home),
            nvm_dir: set(&env.nvm_dir),
            npm_prefix: set(&env.npm_config_prefix),
            others: HOMEBREW.iter().map(PathBuf::from).collect(),
        }
    }

    /// The directories on `PATH`.
    fn on_path(&self) -> Vec<PathBuf> {
        (self.path.iter())
            .flat_map(std::env::split_paths)
            .filter(|dir| !dir.as_os_str().is_empty())
            .collect()
    }

    /// The directories to look in, in order.
    pub fn directories(&self) -> Vec<PathBuf> {
        let mut dirs = self.on_path();
        dirs.extend(self.npm_bin());
        dirs.extend(self.nvm_bins());
        dirs.extend(self.others.iter().cloned());
        dirs
    }

    /// The `bin` directory of npm's global prefix, when it is set.
    fn npm_bin(&self) -> Option<PathBuf> {
        let prefix = match &self.npm_prefix {
            Some(prefix) => prefix.clone(),
            None => {
                let home = self.home.as_ref()?;
                let npmrc = fs::read_to_string(home.join(".npmrc")).ok()?;
                let prefix = npmrc.lines().find_map(|line| {
                    let (key, value) = line.split_once('=')?;
                    (key.trim() == "prefix").then(|| value.trim().trim_matches('"'))
                })?;
                match prefix
                    .strip_prefix("~/")
                    .or_else(|| prefix.strip_prefix("${HOME}/"))
                {
                    Some(rest) => home.join(rest),
                    None => PathBuf::from(prefix),
                }
            }
        };
        Some(prefix.join("bin"))
    }

    /// The `bin` directories of the Node versions nvm installed, the
    /// newest first.
    fn nvm_bins(&self) -> Vec<PathBuf> {
        let nvm = match (&self.nvm_dir, &self.home) {
            (Some(dir), _) => dir.clone(),
            (None, Some(home)) => home.join(".nvm"),
            (None, None) => return Vec::new(),
        };
        let Ok(entries) = fs::read_dir(nvm.join("versions/node")) else {
            return Vec::new();
        };
        let mut versions: Vec<(Vec<u64>, PathBuf)> = (entries.flatten())
            .map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                let number = (name.trim_start_matches('v').split('.'))
                    .map(|part| part.parse().unwrap_or(0))
                    .collect();
                (number, entry.path().join("bin"))
            })
            .collect();
        versions.sort_by(|a, b| b.cmp(a));
        versions.into_iter().map(|(_, dir)| dir).collect()
    }

    /// The first `program` to be found that can be run.
    pub fn find(&self, program: &str) -> Option<PathBuf> {
        (self.directories().into_iter())
            .map(|dir| dir.join(program))
            .find(|path| runnable(path))
    }

    /// A command for `argv` whose program is at `program`. When that is
    /// not on `PATH`, its directory goes first on the `PATH` it runs with:
    /// a script such as `mmdc` finds `node` beside it, as in nvm's and
    /// Homebrew's directories.
    fn command_at(&self, program: &Path, argv: &[&str]) -> Command {
        let mut argv: Vec<String> = argv.iter().map(|arg| (*arg).to_owned()).collect();
        argv[0] = program.to_string_lossy().into_owned();
        let dir = program.parent().unwrap_or(Path::new("/"));
        let mut dirs = self.on_path();
        let path = match dirs.iter().any(|on_path| on_path == dir) {
            true => None,
            false => {
                dirs.insert(0, dir.to_owned());
                std::env::join_paths(dirs).ok()
            }
        };
        Command { argv, path }
    }
}

/// Whether `path` is a file, or a link to one, that may be run.
fn runnable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

/// Where the external command for a language comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// The configuration file gives it.
    Configured(Command),
    /// kiri found the program by itself.
    Found(Command),
    /// kiri looked for a program and did not find it.
    NotFound(&'static str),
    /// kiri would look for the program, but the configuration file says
    /// not to.
    NotLookedFor(&'static str),
    /// Nothing is configured, and kiri does not look: it draws the
    /// language itself, or knows no program for it.
    None,
}

/// The external command that draws `language`: the one `configured`
/// names, or else, in a build without a renderer of its own for the
/// language, the default command line with its program found by `search`,
/// unless `no_search` has the language.
pub fn choose(
    language: &str,
    configured: &BTreeMap<String, Vec<String>>,
    no_search: &BTreeSet<String>,
    search: &Search,
) -> Choice {
    let named = (configured.iter()).find(|(name, _)| name.eq_ignore_ascii_case(language));
    if let Some((_, argv)) = named {
        return Choice::Configured(Command::new(argv.clone()));
    }
    let Some(argv) = default_command(language) else {
        return Choice::None;
    };
    if BuiltIn.draws(language) {
        return Choice::None;
    }
    if no_search
        .iter()
        .any(|name| name.eq_ignore_ascii_case(language))
    {
        return Choice::NotLookedFor(argv[0]);
    }
    match search.find(argv[0]) {
        Some(program) => Choice::Found(search.command_at(&program, argv)),
        None => Choice::NotFound(argv[0]),
    }
}

/// The external command for each language that has one; see `choose`.
pub fn commands(
    configured: &BTreeMap<String, Vec<String>>,
    no_search: &BTreeSet<String>,
    search: &Search,
) -> BTreeMap<String, Command> {
    (super::LANGUAGES.iter())
        .filter_map(
            |&language| match choose(language, configured, no_search, search) {
                Choice::Configured(command) | Choice::Found(command) => {
                    Some((language.to_owned(), command))
                }
                Choice::NotFound(_) | Choice::NotLookedFor(_) | Choice::None => None,
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory of its own for one test, removed when the test ends.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("kiri-find-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }

        /// A program at `path` under the directory, which may be run
        /// unless `mode` says otherwise.
        fn program(&self, path: &str, mode: u32) -> PathBuf {
            let path = self.0.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "#!/bin/sh\n").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
            path
        }

        fn dir(&self, path: &str) -> PathBuf {
            self.0.join(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn looks_on_path_then_in_npm_nvm_and_homebrew() {
        let scratch = Scratch::new("order");
        let path = std::env::join_paths([scratch.dir("bin"), scratch.dir("usr/bin")]).unwrap();
        for version in ["v9.11.2", "v24.16.0", "v18.20.8", "v24.2.0"] {
            fs::create_dir_all(scratch.dir(&format!("home/.nvm/versions/node/{version}/bin")))
                .unwrap();
        }
        let search = Search {
            path: Some(path),
            home: Some(scratch.dir("home")),
            nvm_dir: None,
            npm_prefix: Some(scratch.dir("npm")),
            others: vec![scratch.dir("brew/bin")],
        };
        let nvm = |version| scratch.dir(&format!("home/.nvm/versions/node/{version}/bin"));
        assert_eq!(
            search.directories(),
            [
                scratch.dir("bin"),
                scratch.dir("usr/bin"),
                scratch.dir("npm/bin"),
                nvm("v24.16.0"),
                nvm("v24.2.0"),
                nvm("v18.20.8"),
                nvm("v9.11.2"),
                scratch.dir("brew/bin"),
            ]
        );
        assert_eq!(search.find("mmdc"), None);
        let brew = scratch.program("brew/bin/mmdc", 0o755);
        assert_eq!(search.find("mmdc"), Some(brew));
        let old = scratch.program("home/.nvm/versions/node/v18.20.8/bin/mmdc", 0o755);
        assert_eq!(search.find("mmdc"), Some(old));
        let new = scratch.program("home/.nvm/versions/node/v24.2.0/bin/mmdc", 0o755);
        assert_eq!(search.find("mmdc"), Some(new));
        let npm = scratch.program("npm/bin/mmdc", 0o755);
        assert_eq!(search.find("mmdc"), Some(npm));
        // A file that may not be run, and a directory, do not count.
        scratch.program("usr/bin/mmdc", 0o644);
        fs::create_dir_all(scratch.dir("bin/mmdc")).unwrap();
        assert_eq!(search.find("mmdc"), Some(scratch.dir("npm/bin/mmdc")));
        let usr = scratch.program("usr/bin/mmdc", 0o755);
        assert_eq!(search.find("mmdc"), Some(usr));
    }

    #[test]
    fn nvm_dir_and_npmrc_tell_where_nvm_and_npm_are() {
        let scratch = Scratch::new("nvm-npmrc");
        let home = scratch.dir("home");
        fs::create_dir_all(scratch.dir("nvm/versions/node/v22.0.0/bin")).unwrap();
        fs::create_dir_all(&home).unwrap();
        let search = Search {
            home: Some(home.clone()),
            nvm_dir: Some(scratch.dir("nvm")),
            ..Search::default()
        };
        let nvm = scratch.dir("nvm/versions/node/v22.0.0/bin");
        assert_eq!(search.directories(), std::slice::from_ref(&nvm));
        for (npmrc, prefix) in [
            ("prefix=~/.npm-global\n", home.join(".npm-global")),
            ("color=false\nprefix = \"${HOME}/npm\"\n", home.join("npm")),
            ("prefix=/opt/npm\n", PathBuf::from("/opt/npm")),
        ] {
            fs::write(home.join(".npmrc"), npmrc).unwrap();
            assert_eq!(
                search.directories(),
                [prefix.join("bin"), nvm.clone()],
                "{npmrc}"
            );
        }
    }

    #[test]
    fn the_environment_tells_where_to_look() {
        let env = Environment {
            path: Some("/a:/b".into()),
            home: Some("/h".into()),
            nvm_dir: Some("".into()),
            npm_config_prefix: Some("/p".into()),
            ..Environment::default()
        };
        let search = Search::new(&env);
        assert_eq!(search.path, Some("/a:/b".into()));
        assert_eq!(search.home, Some(PathBuf::from("/h")));
        assert_eq!(search.nvm_dir, None);
        assert_eq!(search.npm_prefix, Some(PathBuf::from("/p")));
        assert_eq!(
            search.others,
            [
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/home/linuxbrew/.linuxbrew/bin")
            ]
        );
    }

    #[test]
    fn mmdc_draws_a_png_at_kiri_s_scale_on_a_transparent_background() {
        let argv = MMDC.join(" ");
        assert!(argv.contains(" -s {scale}"), "{argv}");
        assert!(argv.contains(" -b transparent"), "{argv}");
        // The command that the configuration reference shows.
        let docs = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/config.md");
        let shown = format!("command = {}", serde_json::to_string(MMDC).unwrap());
        let shown = shown.replace("\",\"", "\", \"");
        let docs = fs::read_to_string(docs).unwrap();
        assert_eq!(docs.matches(&shown).count(), 2, "{shown}");
    }

    #[test]
    fn a_program_found_off_path_runs_with_its_directory_on_path() {
        let scratch = Scratch::new("command");
        let search = Search {
            path: Some(
                std::env::join_paths([scratch.dir("bin"), PathBuf::from("/usr/bin")]).unwrap(),
            ),
            others: vec![scratch.dir("brew/bin")],
            ..Search::default()
        };
        let program = scratch.program("brew/bin/mmdc", 0o755);
        let command = search.command_at(&program, MMDC);
        let mut argv: Vec<String> = MMDC.iter().map(|arg| (*arg).to_owned()).collect();
        argv[0] = program.to_string_lossy().into_owned();
        assert_eq!(command.argv, argv);
        let path = std::env::join_paths([
            scratch.dir("brew/bin"),
            scratch.dir("bin"),
            PathBuf::from("/usr/bin"),
        ])
        .unwrap();
        assert_eq!(command.path, Some(path));
        let program = scratch.program("bin/mmdc", 0o755);
        assert_eq!(search.command_at(&program, MMDC).path, None);
    }

    #[test]
    fn looks_for_mmdc_only_without_merman_or_a_configured_command() {
        let scratch = Scratch::new("choose");
        let search = Search {
            path: Some(scratch.dir("bin").into()),
            ..Search::default()
        };
        let none = BTreeMap::new();
        let all = BTreeSet::new();
        let expected = |found| match cfg!(feature = "mermaid") {
            true => Choice::None,
            false => found,
        };
        assert_eq!(
            choose("mermaid", &none, &all, &search),
            expected(Choice::NotFound("mmdc"))
        );
        let program = scratch.program("bin/mmdc", 0o755);
        let found = Choice::Found(search.command_at(&program, MMDC));
        assert_eq!(
            choose("mermaid", &none, &all, &search),
            expected(found.clone())
        );
        assert_eq!(
            commands(&none, &all, &search)
                .into_iter()
                .collect::<Vec<_>>(),
            match found {
                Choice::Found(command) if !cfg!(feature = "mermaid") =>
                    vec![("mermaid".to_owned(), command)],
                _ => vec![],
            }
        );
        let argv = vec![
            "mine".to_owned(),
            "{input}".to_owned(),
            "{output}".to_owned(),
        ];
        let configured = BTreeMap::from([("mermaid".to_owned(), argv.clone())]);
        assert_eq!(
            choose("Mermaid", &configured, &all, &search),
            Choice::Configured(Command::new(argv.clone()))
        );
        assert_eq!(choose("dot", &none, &all, &search), Choice::None);
        // Not looked for when the configuration file says so, though it
        // is there.
        let off = BTreeSet::from(["mermaid".to_owned()]);
        assert_eq!(
            choose("Mermaid", &none, &off, &search),
            expected(Choice::NotLookedFor("mmdc"))
        );
        assert!(commands(&none, &off, &search).is_empty());
        assert_eq!(
            choose("mermaid", &configured, &off, &search),
            Choice::Configured(Command::new(argv))
        );
    }
}
