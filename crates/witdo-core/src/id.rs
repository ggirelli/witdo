use std::fmt;
use std::str::FromStr;

use ulid::Ulid;

/// A task's identifier: a ULID, generated at creation and stable forever, whatever happens to
/// the task's title or file name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskId(Ulid);

impl TaskId {
    /// A new identifier, from the current time and randomness.
    #[must_use]
    pub fn generate() -> Self {
        Self(Ulid::generate())
    }

    #[must_use]
    pub const fn from_ulid(ulid: Ulid) -> Self {
        Self(ulid)
    }

    /// The last `len` characters of the identifier, lowercased. File names use the last 8, and
    /// more when two tasks' suffixes collide.
    #[must_use]
    pub fn suffix(&self, len: usize) -> String {
        let id = self.0.to_string();
        id[id.len() - len.min(id.len())..].to_ascii_lowercase()
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for TaskId {
    type Err = InvalidId;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ulid::from_string(s.trim())
            .map(Self)
            .map_err(|_| InvalidId(s.to_owned()))
    }
}

/// Text that is not a valid task identifier (a 26-character ULID).
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("“{0}” is not a task ID (a 26-character ULID)")]
pub struct InvalidId(pub String);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_prints_the_canonical_form() {
        let id: TaskId = "01JA4XKZ7Q3M8N2P5R4XK9Q2MB".parse().unwrap();
        assert_eq!(id.to_string(), "01JA4XKZ7Q3M8N2P5R4XK9Q2MB");
    }

    #[test]
    fn parsing_ignores_case_and_surrounding_space() {
        let id: TaskId = " 01ja4xkz7q3m8n2p5r4xk9q2mb ".parse().unwrap();
        assert_eq!(id.to_string(), "01JA4XKZ7Q3M8N2P5R4XK9Q2MB");
    }

    #[test]
    fn rejects_text_that_is_not_a_ulid() {
        assert!("prepare-proposal".parse::<TaskId>().is_err());
        assert!("".parse::<TaskId>().is_err());
    }

    #[test]
    fn suffix_is_the_lowercased_end_of_the_id() {
        let id: TaskId = "01JA4XKZ7Q3M8N2P5R4XK9Q2MB".parse().unwrap();
        assert_eq!(id.suffix(8), "4xk9q2mb");
        assert_eq!(id.suffix(10), "5r4xk9q2mb");
        assert_eq!(id.suffix(99).len(), 26);
    }

    #[test]
    fn generated_ids_are_distinct() {
        assert_ne!(TaskId::generate(), TaskId::generate());
    }
}
