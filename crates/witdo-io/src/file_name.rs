//! Task file names: `<slug>--<short-id>.md`. They are for people; witdo finds tasks by their ID.

use witdo_core::{TaskId, Title};

/// The longest slug, in characters.
const MAX_SLUG: usize = 60;
/// The short ID's usual length; it grows when two tasks' short IDs collide.
const SHORT_ID: usize = 8;
/// What a title with no letters or digits becomes.
const FALLBACK_SLUG: &str = "task";

/// The title lowercased and transliterated to ASCII, with runs of anything but letters and
/// digits turned into a single `-`, at most 60 characters long.
#[must_use]
pub fn slug(title: &str) -> String {
    let ascii = deunicode::deunicode(title).to_ascii_lowercase();
    let mut slug = String::with_capacity(ascii.len().min(MAX_SLUG));
    for word in ascii
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
    {
        if !slug.is_empty() {
            slug.push('-');
        }
        slug.push_str(word);
        if slug.len() >= MAX_SLUG {
            break;
        }
    }
    slug.truncate(MAX_SLUG);
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        FALLBACK_SLUG.to_owned()
    } else {
        slug.to_owned()
    }
}

/// The file name for a task, with the shortest short ID (8 characters or more) that gives a
/// name `is_taken` says is free.
#[must_use]
pub fn file_name(title: &Title, id: TaskId, is_taken: impl Fn(&str) -> bool) -> String {
    let slug = slug(title.as_str());
    let mut name = String::new();
    for len in SHORT_ID..=26 {
        name = format!("{slug}--{}.md", id.suffix(len));
        if !is_taken(&name) {
            break;
        }
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> TaskId {
        "01JA4XKZ7Q3M8N2P5R4XK9Q2MB".parse().unwrap()
    }

    fn title(text: &str) -> Title {
        Title::new(text).unwrap()
    }

    #[test]
    fn slugs_are_lowercase_ascii_words_joined_by_dashes() {
        assert_eq!(slug("Prepare project proposal"), "prepare-project-proposal");
        assert_eq!(
            slug("  Call the bank!!  (re: loan) "),
            "call-the-bank-re-loan"
        );
        assert_eq!(slug("Crème brûlée für Jürgen"), "creme-brulee-fur-jurgen");
        assert_eq!(slug("Q4: 2026 plan"), "q4-2026-plan");
    }

    #[test]
    fn slugs_fall_back_when_nothing_is_left() {
        assert_eq!(slug("!!!"), "task");
        assert_eq!(slug("—"), "task");
    }

    #[test]
    fn slugs_are_at_most_60_characters_without_a_trailing_dash() {
        let long = "word ".repeat(30);
        let slug = slug(&long);
        assert!(slug.len() <= 60, "{slug}");
        assert!(!slug.ends_with('-'));
        assert_eq!(super::slug(&"a".repeat(100)).len(), 60);
    }

    #[test]
    fn file_names_end_with_the_short_id() {
        let name = file_name(&title("Prepare project proposal"), id(), |_| false);
        assert_eq!(name, "prepare-project-proposal--4xk9q2mb.md");
    }

    #[test]
    fn file_names_grow_the_short_id_when_taken() {
        let taken = ["call-the-bank--4xk9q2mb.md", "call-the-bank--r4xk9q2mb.md"];
        let name = file_name(&title("Call the bank"), id(), |name| taken.contains(&name));
        assert_eq!(name, "call-the-bank--5r4xk9q2mb.md");
    }
}
