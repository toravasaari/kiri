//! The reports that kiri writes itself instead of reading them from a file:
//! `--doctor`, which tells what kiri found out about the terminal, the
//! configuration and the theme, and `--list-emojis`. Each is rendered for a
//! terminal and written as plain text otherwise.
//!
//! What does not belong here is the content of a report, which the library
//! makes (`kiri::doctor` and `kiri::list`), nor the showing of a document,
//! which goes through the same route as a file, `frontend::show_document`.
//!
//! A new report that kiri writes itself goes here, with its content in the
//! library.

use std::io::Write;

use kiri::doctor::{Facts, Output};
use kiri::frontend::{Failure, Platform, show_document, shown};
use kiri::session::plan;
use kiri::typeset::theme::Theme;

use crate::Real;
use crate::cli::{Cli, followed, options};

/// Writes the `--doctor` report: rendered for a terminal, as Markdown
/// otherwise or with `--raw`.
/// Problems with the configuration file and the theme are part of the
/// report instead of errors, and kiri goes on without them.
pub(crate) fn doctor(real: &Real, cli: &Cli) -> Result<(), Failure> {
    let env = real.environment();
    let (config, config_error) = match kiri::config::load(env) {
        Ok(config) => (config, None),
        Err(error) => (Default::default(), Some(error)),
    };
    let choices = kiri::config::choose(&options(cli), env, &config);
    let (theme, theme_error) = match kiri::config::load_theme(&choices.theme.value) {
        Ok(theme) => (theme, None),
        Err(error) => (Theme::default(), Some(error)),
    };
    let passthrough = choices.passthrough.value;
    let detection = kiri::term::examine(choices.mode.value, choices.width.value, env, passthrough);
    let output = match (cli.raw, real.stdout_is_terminal()) {
        (true, _) => Output::Raw,
        (false, true) => Output::Rendered,
        (false, false) => Output::Markdown,
    };
    let markdown = kiri::doctor::report(&Facts {
        env,
        detection: &detection,
        choices: &choices,
        theme: &theme,
        config_error,
        theme_error,
        output,
    });
    if output != Output::Rendered {
        let mut out = real.stdout();
        return shown(
            out.write_all(markdown.as_bytes())
                .and_then(|()| out.flush()),
        );
    }
    let document = kiri::typeset::parse::parse(&markdown);
    let mut settings = choices.with_theme(theme);
    settings.paging = plan::only_always(settings.paging);
    let follow = followed(cli);
    show_document(
        real,
        follow,
        choices.reading_named(),
        "kiri --doctor",
        document,
        settings,
        detection.caps,
    )
}

/// Writes the emoji shortcodes: a rendered document for a terminal, one
/// line per emoji otherwise.
pub(crate) fn list_emojis(real: &Real, cli: &Cli) -> Result<(), Failure> {
    if !real.stdout_is_terminal() {
        let mut out = real.stdout();
        let lines = kiri::list::emoji_lines();
        return shown(out.write_all(lines.as_bytes()).and_then(|()| out.flush()));
    }
    let env = real.environment();
    let resolved = kiri::config::resolve_for_run(&options(cli), env);
    (resolved.warnings.iter())
        .for_each(|warning| eprintln!("kiri: {}", kiri::term::printable(&warning.to_string())));
    let settings = resolved.settings.map_err(|(error, _)| error)?;
    let caps = real.capabilities(settings.mode, settings.width, settings.passthrough);
    let document = kiri::typeset::parse::parse(&kiri::list::emoji_document());
    let follow = followed(cli);
    show_document(
        real,
        follow,
        resolved.reading_named,
        "kiri --list-emojis",
        document,
        settings,
        caps,
    )
}
