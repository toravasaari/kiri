//! Running `tmux` and reading what it says about the pane kiri runs in:
//! the platform side of running inside tmux. The wrapping of kitty's
//! sequences for tmux is in `src/present/tmux.rs`.

use std::process::{Command, Stdio};

use crate::config::Environment;
use crate::present::tmux::Pane;

/// Runs `tmux` with the arguments: its output, trimmed, when it succeeds,
/// and `None` when it fails or cannot run.
pub type Run<'a> = &'a dyn Fn(&[&str]) -> Option<String>;

/// Runs the `tmux` on `PATH`, as [`Run`] says.
pub fn run(args: &[&str]) -> Option<String> {
    let out = Command::new("tmux")
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    out.status.success().then(|| text.trim().to_owned())
}

/// Something unique to this run of kiri, to mix into its image ids: every
/// pane of a kitty window shares kitty's images, and kiri runs in other
/// panes, also in other containers, whose process ids may be the same.
pub fn run_id() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    // Each `RandomState` has keys of its own, random for each process.
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u32(std::process::id());
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    hasher.write_u128(now.map_or(0, |now| now.as_nanos()));
    hasher.finish()
}

/// Whether kiri runs inside tmux: `TMUX` set and not empty.
pub fn in_tmux(env: &Environment) -> bool {
    env.tmux.as_ref().is_some_and(|value| !value.is_empty())
}

/// Asks tmux through `run` for the outer terminal, the pane's
/// `allow-passthrough`, `focus-events` and whether tmux passes links, and
/// turns `allow-passthrough` on when it is `off`, as
/// `kitten icat` does; `on` and `all` stay as they are. The pane is
/// `TMUX_PANE`, or tmux's current one without it.
pub fn prepare(env: &Environment, run: Run) -> Pane {
    let pane = env.tmux_pane.as_deref().and_then(|pane| pane.to_str());
    let target: &[&str] = match pane {
        Some(pane) => &["-t", pane],
        None => &[],
    };
    let ask = "#{client_termtype}|#{allow-passthrough}|#{focus-events}\
               |#{client_termfeatures}|#{client_termname}";
    let answer = run(&[&["display-message", "-p"], target, &[ask]].concat());
    let known = |field: &str| (!field.is_empty()).then(|| field.to_owned());
    let mut fields = answer.as_deref().map(|answer| answer.splitn(5, '|'));
    let mut next = || fields.as_mut().and_then(Iterator::next);
    let (client, passthrough) = match (next(), next()) {
        (Some(client), Some(passthrough)) => (known(client), known(passthrough)),
        _ => (None, None),
    };
    // A flag in a format is 1 or 0.
    let focus_events = match next() {
        Some("1" | "on") => Some(true),
        Some("0" | "off") => Some(false),
        _ => None,
    };
    // The features are a list such as `bpaste,clipboard,hyperlinks,focus`.
    let hyperlinks = next()
        .and_then(known)
        .map(|features| features.split(',').any(|feature| feature == "hyperlinks"));
    let term_name = next().and_then(known);
    let turned_on = passthrough.as_deref() == Some("off")
        && run(&[&["set-option", "-p"], target, &["allow-passthrough", "on"]].concat()).is_some();
    Pane {
        client,
        passthrough,
        turned_on,
        focus_events,
        hyperlinks,
        term_name,
    }
}
#[cfg(test)]
mod tests;
