//! The reports that kiri writes itself instead of reading them from a file:
//! `--doctor`, which tells what kiri found out about the terminal, the
//! configuration and the theme, and `--list-emojis`. Each is rendered for a
//! terminal and written as plain text otherwise.
//!
//! What does not belong here is the content of a report, which the library
//! makes (`kiri::doctor` and `kiri::list`), nor the showing of a document,
//! which goes through the same route as a file in `main.rs`.
//!
//! A new report that kiri writes itself goes here, with its content in the
//! library.

use std::io::{self, Write};

use kiri::config::Environment;
use kiri::doctor::{Facts, Output};
use kiri::frontend::{Failure, show_document, shown};
use kiri::session::plan;
use kiri::typeset::theme::Theme;

use crate::cli::{Cli, followed, options};
use crate::settings;

/// Writes the `--doctor` report: rendered for a terminal, as Markdown
/// otherwise or with `--raw`.
/// Problems with the configuration file and the theme are part of the
/// report instead of errors, and kiri goes on without them.
pub(crate) fn doctor(cli: &Cli) -> Result<(), Failure> {
    let env = Environment::from_process();
    let (config, config_error) = match kiri::config::load(&env) {
        Ok(config) => (config, None),
        Err(error) => (Default::default(), Some(error)),
    };
    let choices = kiri::config::choose(&options(cli), &env, &config);
    let (theme, theme_error) = match kiri::config::load_theme(&choices.theme.value) {
        Ok(theme) => (theme, None),
        Err(error) => (Theme::default(), Some(error)),
    };
    let passthrough = choices.passthrough.value;
    let detection = kiri::term::examine(choices.mode.value, choices.width.value, &env, passthrough);
    let output = match (cli.raw, kiri::term::stdout_is_terminal()) {
        (true, _) => Output::Raw,
        (false, true) => Output::Rendered,
        (false, false) => Output::Markdown,
    };
    let markdown = kiri::doctor::report(&Facts {
        env: &env,
        detection: &detection,
        choices: &choices,
        theme: &theme,
        config_error,
        theme_error,
        output,
    });
    if output != Output::Rendered {
        let mut out = io::stdout().lock();
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
        &env,
        follow,
        "kiri --doctor",
        document,
        settings,
        detection.caps,
    )
}

/// Writes the emoji shortcodes: a rendered document for a terminal, one
/// line per emoji otherwise.
pub(crate) fn list_emojis(cli: &Cli) -> Result<(), Failure> {
    if !kiri::term::stdout_is_terminal() {
        let mut out = io::stdout().lock();
        let lines = kiri::list::emoji_lines();
        return shown(out.write_all(lines.as_bytes()).and_then(|()| out.flush()));
    }
    let env = Environment::from_process();
    let (warnings, settings) = settings(cli, &env);
    (warnings.iter()).for_each(|warning| eprintln!("kiri: {warning}"));
    let settings = settings?;
    let caps = kiri::term::capabilities(settings.mode, settings.width, &env, settings.passthrough);
    let document = kiri::typeset::parse::parse(&kiri::list::emoji_document());
    let follow = followed(cli);
    show_document(&env, follow, "kiri --list-emojis", document, settings, caps)
}
