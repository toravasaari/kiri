//! The pager's commands as data: each has its topic, its keys, its text in
//! the help panel and the action it sends. A new key is one entry in
//! [`COMMANDS`] and the pager's answer to its action; the help panel and
//! the lookup of keys read the same table.

use crate::present::keys::Key;
use crate::session::bands::Step;
use crate::session::view::Motion;

/// What a command is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Topic {
    Moving,
    Searching,
    /// Moving between files, which only several files have.
    Files,
    Links,
    View,
    Help,
}

/// What a key asks the pager to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    Move(Motion),
    /// Goes down a line, or copies the link selected on the screen.
    Enter,
    /// Opens the search prompt.
    Search,
    /// Moves to the next match of the last search.
    Next,
    /// Moves to the previous match of the last search.
    Previous,
    /// Closes what is open, the innermost first: the help, the prompt,
    /// the colon, and at last the search's marks and the selected link.
    Dismiss,
    /// Moves to the start of the next file.
    NextFile,
    /// Moves to the start of the file the view is in, or of the one
    /// before when the view is there already.
    PreviousFile,
    /// Reads the document again.
    Reload,
    /// Draws the screen again, with every image on it sent again.
    Redraw,
    /// Follows the document, from off to on to auto and back to off.
    Follow,
    /// Turns reading mode on or off.
    Reading,
    /// Scrolls a wide block sideways, a little or by most of its width.
    Sideways(Step),
    /// Selects the next or the previous link on the screen.
    SelectLink {
        back: bool,
    },
    /// Pauses the animation selected, or the ones playing on the screen,
    /// or plays them again.
    Pause,
    /// Switches wide blocks between wrapping and scrolling.
    Wide,
    /// Turns the page's background colour on or off.
    Background,
    Help,
    Quit,
    /// The left button pressed on row `row` of the window, counted from 1.
    Click {
        row: u16,
    },
    /// A notch of the sideways wheel, to the right or the left, or of
    /// the wheel with Shift, down or up, on row `row`.
    Wheel {
        right: bool,
        shift: bool,
        row: u16,
    },
}

impl Action {
    /// Whether the action comes from the mouse, which does nothing while
    /// the help or the prompt is open.
    pub fn is_pointer(self) -> bool {
        matches!(self, Action::Click { .. } | Action::Wheel { .. })
    }
}

/// One command: the keys that send an action, and how the help panel
/// lists them.
#[derive(Debug)]
pub struct Command {
    pub topic: Topic,
    pub keys: &'static [Key],
    /// The keys most people use, shortest first. Empty for a command that
    /// the help panel lists in the row of the one before it, or not at
    /// all.
    pub names: &'static str,
    /// What the keys do.
    pub what: &'static str,
    pub(crate) action: Action,
    /// The settings, by their key in the configuration file, that change
    /// what the command does. Nothing reads them yet: the menu will, to
    /// show a command's settings beside it.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "read by the menu, not yet built")
    )]
    pub settings: &'static [&'static str],
}

impl Command {
    /// A command that the help panel lists as `names`, `what`.
    const fn new(
        topic: Topic,
        keys: &'static [Key],
        names: &'static str,
        what: &'static str,
        action: Action,
    ) -> Self {
        Command {
            topic,
            keys,
            names,
            what,
            action,
            settings: &[],
        }
    }

    /// More keys for the command above, which its row of the help panel
    /// already names, as the other direction of a pair.
    const fn also(topic: Topic, keys: &'static [Key], action: Action) -> Self {
        Self::new(topic, keys, "", "", action)
    }

    const fn setting(self, settings: &'static [&'static str]) -> Self {
        Command { settings, ..self }
    }

    /// Whether the help panel has a row for the command.
    pub fn listed(&self) -> bool {
        !self.names.is_empty()
    }
}

/// The key that starts a command, as in `less`, with several files: the
/// key after it is the command's, as [`Key::Command`].
pub const COMMAND: Key = Key::Char(':');

/// How many lines a notch of the mouse wheel moves.
pub const WHEEL: usize = 3;

/// Every command the pager knows, in the order the help lists them.
pub const COMMANDS: &[Command] = {
    use Action::{
        Background, Enter, Follow, Help, Move, Next, NextFile, Pause, Previous, PreviousFile, Quit,
        Reading, Redraw, Reload, Search, SelectLink, Sideways, Wide,
    };
    use Key::{Char, Ctrl};
    use Motion::*;
    use Topic::{Files, Help as HelpTopic, Links, Moving, Searching, View};
    &[
        Command::new(
            Moving,
            &[Char('j'), Char('e'), Ctrl('e'), Ctrl('n'), Key::Down],
            "j ↓ Enter",
            "down a line",
            Move(Down(1)),
        ),
        Command::also(Moving, &[Key::Enter], Enter),
        Command::new(
            Moving,
            &[
                Char('k'),
                Char('y'),
                Ctrl('y'),
                Ctrl('p'),
                Ctrl('k'),
                Key::Up,
            ],
            "k ↑",
            "up a line",
            Move(Up(1)),
        ),
        Command::new(
            Moving,
            &[Char(' '), Char('f'), Ctrl('f'), Ctrl('v'), Key::PageDown],
            "Space f PgDn",
            "down a page",
            Move(PageDown),
        ),
        Command::new(
            Moving,
            &[Char('b'), Ctrl('b'), Key::PageUp],
            "b PgUp",
            "up a page",
            Move(PageUp),
        ),
        Command::new(
            Moving,
            &[Char('d'), Ctrl('d')],
            "d",
            "down half a page",
            Move(HalfPageDown),
        ),
        Command::new(
            Moving,
            &[Char('u'), Ctrl('u')],
            "u",
            "up half a page",
            Move(HalfPageUp),
        ),
        Command::new(
            Moving,
            &[Char('g'), Char('<'), Key::Home],
            "g Home",
            "top",
            Move(Start),
        ),
        Command::new(
            Moving,
            &[Char('G'), Char('>'), Key::End],
            "G End",
            "bottom",
            Move(End),
        ),
        Command::also(Moving, &[Key::WheelUp], Move(Up(WHEEL))).setting(&["mouse"]),
        Command::also(Moving, &[Key::WheelDown], Move(Down(WHEEL))).setting(&["mouse"]),
        Command::new(
            Files,
            &[Key::Command('n'), Key::ShiftPageDown],
            ":n Shift+PgDn",
            "next file",
            NextFile,
        ),
        Command::new(
            Files,
            &[Key::Command('p'), Key::ShiftPageUp],
            ":p Shift+PgUp",
            "previous file",
            PreviousFile,
        ),
        Command::new(Searching, &[Char('/')], "/", "search", Search).setting(&["search_case"]),
        Command::new(Searching, &[Char('n')], "n", "next match", Next).setting(&["search_case"]),
        Command::new(Searching, &[Char('N')], "N", "previous match", Previous)
            .setting(&["search_case"]),
        Command::new(
            Searching,
            &[Key::Escape],
            "Esc",
            "hide the search",
            Action::Dismiss,
        ),
        Command::new(View, &[Char('p')], "p", "pause or play animations", Pause)
            .setting(&["animations"]),
        Command::new(View, &[Char('r')], "r", "reload the file", Reload),
        Command::new(View, &[Ctrl('l')], "Ctrl+L", "redraw the screen", Redraw),
        Command::new(View, &[Char('F')], "F", "follow: on, auto, off", Follow)
            .setting(&["follow", "follow_jump"]),
        Command::new(View, &[Char('M')], "M", "reading mode: on, off", Reading),
        Command::new(
            View,
            &[Key::Left],
            "← →",
            "scroll a wide block",
            Sideways(Step::Left),
        ),
        Command::also(View, &[Key::Right], Sideways(Step::Right)),
        Command::new(
            View,
            &[Char('H')],
            "H L",
            "scroll it by its width",
            Sideways(Step::PageLeft),
        ),
        Command::also(View, &[Char('L')], Sideways(Step::PageRight)),
        Command::new(
            Links,
            &[Ctrl('i')],
            "Tab Shift+Tab",
            "select a link, Enter copies it",
            SelectLink { back: false },
        ),
        Command::also(Links, &[Key::BackTab], SelectLink { back: true }),
        Command::new(View, &[Char('W')], "W", "wide blocks: wrap, scroll", Wide),
        Command::new(View, &[Char('B')], "B", "background: on, off", Background),
        Command::new(HelpTopic, &[Char('?'), Char('h')], "? h", "this help", Help),
        Command::new(
            HelpTopic,
            &[Char('q'), Char('Q'), Ctrl('c')],
            "q",
            "quit",
            Quit,
        ),
    ]
};

/// What `key` does; `None` for a key without a command. A click and the
/// sideways wheel carry where they happened, so they are no entries of
/// [`COMMANDS`].
pub(crate) fn action(key: Key) -> Option<Action> {
    match key {
        Key::Click { row, .. } => Some(Action::Click { row }),
        Key::Sideways {
            right, shift, row, ..
        } => Some(Action::Wheel { right, shift, row }),
        key => (COMMANDS.iter())
            .find(|command| command.keys.contains(&key))
            .map(|command| command.action),
    }
}

/// The key the status line shows for `action`: the first of the names
/// in the help panel's row for its command.
pub(crate) fn hint(action: Action) -> &'static str {
    (COMMANDS.iter())
        .find(|command| command.action == action && command.listed())
        .and_then(|command| command.names.split(' ').next())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SETTINGS;

    #[test]
    fn the_status_line_hints_are_the_first_names_of_their_commands() {
        assert_eq!(hint(Action::Help), "?");
        assert_eq!(hint(Action::Quit), "q");
        assert_eq!(hint(Action::Follow), "F");
        assert_eq!(hint(Action::Reading), "M");
    }

    #[test]
    fn a_key_is_in_one_command_only() {
        let mut seen: Vec<Key> = Vec::new();
        for command in COMMANDS {
            for key in command.keys {
                assert!(!seen.contains(key), "{key:?} twice");
                seen.push(*key);
            }
        }
    }

    #[test]
    fn the_settings_a_command_names_are_settings() {
        let keys: Vec<&str> = SETTINGS.iter().filter_map(|entry| entry.key).collect();
        for command in COMMANDS {
            for name in command.settings {
                assert!(keys.contains(name), "{name} of {}", command.names);
            }
        }
    }

    #[test]
    fn a_listed_command_has_its_text_and_an_unlisted_one_has_none() {
        for command in COMMANDS {
            assert_eq!(command.listed(), !command.what.is_empty(), "{command:?}");
        }
    }

    #[test]
    fn the_mouse_sends_actions_with_where_it_happened() {
        let click = Key::Click { column: 5, row: 3 };
        assert_eq!(action(click), Some(Action::Click { row: 3 }));
        let wheel = Key::Sideways {
            right: true,
            shift: false,
            column: 1,
            row: 7,
        };
        assert_eq!(
            action(wheel),
            Some(Action::Wheel {
                right: true,
                shift: false,
                row: 7
            })
        );
        assert!(action(click).is_some_and(Action::is_pointer));
        assert!(!action(Key::Char('j')).is_some_and(Action::is_pointer));
    }

    #[test]
    fn keys_move_as_in_less() {
        let moves = |keys: &[Key], motion| {
            for &key in keys {
                assert_eq!(action(key), Some(Action::Move(motion)), "{key:?}");
            }
        };
        let down = [
            Key::Char('j'),
            Key::Char('e'),
            Key::Ctrl('e'),
            Key::Ctrl('n'),
            Key::Down,
        ];
        moves(&down, Motion::Down(1));
        let up = [
            Key::Char('k'),
            Key::Char('y'),
            Key::Ctrl('y'),
            Key::Ctrl('p'),
            Key::Ctrl('k'),
            Key::Up,
        ];
        moves(&up, Motion::Up(1));
        let page_down = [
            Key::Char(' '),
            Key::Char('f'),
            Key::Ctrl('f'),
            Key::Ctrl('v'),
            Key::PageDown,
        ];
        moves(&page_down, Motion::PageDown);
        moves(
            &[Key::Char('b'), Key::Ctrl('b'), Key::PageUp],
            Motion::PageUp,
        );
        moves(&[Key::Char('d'), Key::Ctrl('d')], Motion::HalfPageDown);
        moves(&[Key::Char('u'), Key::Ctrl('u')], Motion::HalfPageUp);
        moves(&[Key::Char('g'), Key::Char('<'), Key::Home], Motion::Start);
        moves(&[Key::Char('G'), Key::Char('>'), Key::End], Motion::End);
    }

    #[test]
    fn question_mark_and_h_open_the_help() {
        for key in [Key::Char('?'), Key::Char('h')] {
            assert_eq!(action(key), Some(Action::Help), "{key:?}");
        }
    }

    #[test]
    fn the_help_names_keys_of_their_own_binding() {
        for (at, command) in COMMANDS.iter().enumerate().filter(|(_, c)| c.listed()) {
            // The row also holds the keys of the commands after it that
            // have no row of their own.
            let row: Vec<Key> = COMMANDS[at..]
                .iter()
                .enumerate()
                .take_while(|&(i, c)| i == 0 || !c.listed())
                .flat_map(|(_, c)| c.keys.iter().copied())
                .collect();
            for name in command.names.split(' ') {
                let key = match name {
                    "↓" => Key::Down,
                    "↑" => Key::Up,
                    "←" => Key::Left,
                    "→" => Key::Right,
                    "Tab" => Key::Ctrl('i'),
                    "Shift+Tab" => Key::BackTab,
                    "Enter" => Key::Enter,
                    "Esc" => Key::Escape,
                    "Space" => Key::Char(' '),
                    "PgDn" => Key::PageDown,
                    "PgUp" => Key::PageUp,
                    "Home" => Key::Home,
                    "End" => Key::End,
                    ":n" => Key::Command('n'),
                    ":p" => Key::Command('p'),
                    "Shift+PgDn" => Key::ShiftPageDown,
                    "Shift+PgUp" => Key::ShiftPageUp,
                    "Ctrl+L" => Key::Ctrl('l'),
                    name if name.chars().count() == 1 => Key::Char(name.chars().next().unwrap()),
                    name => panic!("unknown key name {name}"),
                };
                assert!(row.contains(&key), "{name} in {}", command.names);
            }
        }
        assert!(
            COMMANDS
                .iter()
                .any(|command| command.action == Action::Help)
        );
        assert!(
            COMMANDS
                .iter()
                .any(|command| command.action == Action::Quit)
        );
    }

    #[test]
    fn enter_and_the_wheel_have_actions_of_their_own() {
        assert_eq!(action(Key::Enter), Some(Action::Enter));
    }

    #[test]
    fn a_slash_searches_and_n_moves_between_matches() {
        assert_eq!(action(Key::Char('/')), Some(Action::Search));
        assert_eq!(action(Key::Char('n')), Some(Action::Next));
        assert_eq!(action(Key::Char('N')), Some(Action::Previous));
        assert_eq!(action(Key::Escape), Some(Action::Dismiss));
    }

    #[test]
    fn colon_n_and_colon_p_move_between_files() {
        assert_eq!(action(Key::Command('n')), Some(Action::NextFile));
        assert_eq!(action(Key::Command('p')), Some(Action::PreviousFile));
        // On its own, p is for animations, not for files.
        assert_eq!(action(Key::Char('p')), Some(Action::Pause));
        assert_eq!(action(COMMAND), None);
        assert_eq!(action(Key::Command('x')), None);
    }

    #[test]
    fn shift_page_down_and_shift_page_up_move_between_files() {
        assert_eq!(action(Key::ShiftPageDown), Some(Action::NextFile));
        assert_eq!(action(Key::ShiftPageUp), Some(Action::PreviousFile));
    }

    #[test]
    fn r_reloads_and_capital_f_follows() {
        assert_eq!(action(Key::Char('r')), Some(Action::Reload));
        assert_eq!(action(Key::Char('F')), Some(Action::Follow));
    }

    #[test]
    fn q_and_ctrl_c_quit() {
        for key in [Key::Char('q'), Key::Char('Q'), Key::Ctrl('c')] {
            assert_eq!(action(key), Some(Action::Quit), "{key:?}");
        }
    }

    #[test]
    fn other_keys_do_nothing() {
        for key in [Key::Char('x'), Key::Other, Key::Ctrl('z')] {
            assert_eq!(action(key), None, "{key:?}");
        }
    }

    #[test]
    fn ctrl_l_redraws_and_the_wheel_moves_three_lines() {
        assert_eq!(action(Key::Ctrl('l')), Some(Action::Redraw));
        assert_eq!(action(Key::WheelDown), Some(Action::Move(Motion::Down(3))));
        assert_eq!(action(Key::WheelUp), Some(Action::Move(Motion::Up(3))));
    }

    #[test]
    fn what_the_terminal_sends_besides_keys_is_no_command() {
        for key in [
            Key::CellSize(10, 24),
            Key::FocusIn,
            Key::FocusOut,
            Key::Mouse,
            Key::Alt('q'),
            Key::Alt('j'),
        ] {
            assert_eq!(action(key), None, "{key:?}");
        }
    }
}
