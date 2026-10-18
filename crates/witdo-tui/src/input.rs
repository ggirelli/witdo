//! A single-line text field.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextInput {
    text: String,
    /// In characters, from the start.
    cursor: usize,
}

impl TextInput {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            cursor: text.chars().count(),
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Where the cursor is, in characters from the start.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Applies an editing key; returns whether it was one.
    pub fn handle(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('u') if ctrl => {
                self.text.clear();
                self.cursor = 0;
            }
            KeyCode::Char('a') if ctrl => self.cursor = 0,
            KeyCode::Char('e') if ctrl => self.cursor = self.len(),
            KeyCode::Char(c) if !ctrl => {
                let at = self.byte_index(self.cursor);
                self.text.insert(at, c);
                self.cursor += 1;
            }
            KeyCode::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                let at = self.byte_index(self.cursor);
                self.text.remove(at);
            }
            KeyCode::Delete if self.cursor < self.len() => {
                let at = self.byte_index(self.cursor);
                self.text.remove(at);
            }
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.len()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.len(),
            KeyCode::Backspace | KeyCode::Delete => {}
            _ => return false,
        }
        true
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    fn byte_index(&self, chars: usize) -> usize {
        self.text
            .char_indices()
            .nth(chars)
            .map_or(self.text.len(), |(index, _)| index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::from(code)
    }

    fn typed(input: &mut TextInput, text: &str) {
        for c in text.chars() {
            input.handle(key(KeyCode::Char(c)));
        }
    }

    #[test]
    fn types_and_edits_at_the_cursor() {
        let mut input = TextInput::default();
        typed(&mut input, "Cal bank");
        for _ in 0..5 {
            input.handle(key(KeyCode::Left));
        }
        typed(&mut input, "l");
        assert_eq!(input.text(), "Call bank");
        input.handle(key(KeyCode::End));
        input.handle(key(KeyCode::Backspace));
        assert_eq!(input.text(), "Call ban");
        input.handle(key(KeyCode::Home));
        input.handle(key(KeyCode::Delete));
        assert_eq!(input.text(), "all ban");
    }

    #[test]
    fn handles_characters_beyond_ascii() {
        let mut input = TextInput::new("Crème");
        input.handle(key(KeyCode::Left));
        input.handle(key(KeyCode::Backspace));
        assert_eq!(input.text(), "Crèe");
        assert_eq!(input.cursor(), 3);
    }

    #[test]
    fn ctrl_u_clears() {
        let mut input = TextInput::new("something");
        input.handle(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!(input.text(), "");
    }

    #[test]
    fn other_keys_are_not_editing() {
        let mut input = TextInput::default();
        assert!(!input.handle(key(KeyCode::Enter)));
        assert!(!input.handle(key(KeyCode::Esc)));
    }
}
