//! The search prompt: the term as the reader types it on the status line.

use super::keys::Key;

/// A term being typed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Prompt {
    term: String,
}

/// What a key does to the prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// The prompt stays open.
    Typing,
    /// The prompt closes without a search.
    Cancel,
    /// The prompt closes, and the pager searches for this term.
    Search(String),
    /// The prompt closes, and the pager repeats the last search, if any.
    Repeat,
}

impl Prompt {
    /// The term typed so far.
    pub fn term(&self) -> &str {
        &self.term
    }

    /// Edits the term with `key`: Enter searches, or repeats the last
    /// search when there is no term, as in `less`; Escape and Ctrl+C cancel,
    /// Backspace deletes the last character and cancels when there is
    /// none, Ctrl+U clears the term. Other keys type their character, if
    /// they have one.
    pub fn edit(&mut self, key: Key) -> Edit {
        match key {
            Key::Enter if self.term.is_empty() => Edit::Repeat,
            Key::Enter => Edit::Search(std::mem::take(&mut self.term)),
            Key::Escape | Key::Ctrl('c') => Edit::Cancel,
            Key::Backspace => match self.term.pop() {
                Some(_) => Edit::Typing,
                None => Edit::Cancel,
            },
            Key::Ctrl('u') => {
                self.term.clear();
                Edit::Typing
            }
            Key::Char(c) => {
                self.term.push(c);
                Edit::Typing
            }
            _ => Edit::Typing,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(keys: &[Key]) -> (Prompt, Edit) {
        let mut prompt = Prompt::default();
        let mut edit = Edit::Typing;
        for &key in keys {
            edit = prompt.edit(key);
        }
        (prompt, edit)
    }

    #[test]
    fn characters_are_typed_and_enter_searches() {
        let (prompt, edit) = typed(&[Key::Char('k'), Key::Char('ä')]);
        assert_eq!((prompt.term(), edit), ("kä", Edit::Typing));
        let (_, edit) = typed(&[Key::Char('k'), Key::Enter]);
        assert_eq!(edit, Edit::Search("k".to_owned()));
    }

    #[test]
    fn enter_on_an_empty_prompt_repeats() {
        assert_eq!(typed(&[Key::Enter]).1, Edit::Repeat);
    }

    #[test]
    fn escape_and_ctrl_c_cancel() {
        for key in [Key::Escape, Key::Ctrl('c')] {
            assert_eq!(typed(&[Key::Char('k'), key]).1, Edit::Cancel, "{key:?}");
        }
    }

    #[test]
    fn backspace_deletes_a_character_and_then_cancels() {
        let (prompt, edit) = typed(&[Key::Char('k'), Key::Char('ä'), Key::Backspace]);
        assert_eq!((prompt.term(), edit), ("k", Edit::Typing));
        let (_, edit) = typed(&[Key::Char('k'), Key::Backspace, Key::Backspace]);
        assert_eq!(edit, Edit::Cancel);
    }

    #[test]
    fn ctrl_u_clears_the_term() {
        let (prompt, edit) = typed(&[Key::Char('k'), Key::Ctrl('u')]);
        assert_eq!((prompt.term(), edit), ("", Edit::Typing));
    }

    #[test]
    fn other_keys_type_nothing() {
        let (prompt, edit) = typed(&[Key::Up, Key::Alt('k'), Key::Ctrl('f'), Key::Other]);
        assert_eq!((prompt.term(), edit), ("", Edit::Typing));
    }
}
