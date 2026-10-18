use std::fmt;
use std::hash::{Hash, Hasher};
use std::str::FromStr;

use jiff::Timestamp;
use jiff::civil::Date;

use crate::TaskId;

/// Where a task is in its life. `Inbox`, `Todo` and `InProgress` are open; `Done` and
/// `Cancelled` are closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Status {
    Inbox,
    Todo,
    InProgress,
    Done,
    Cancelled,
}

impl Status {
    pub const ALL: [Self; 5] = [
        Self::Inbox,
        Self::Todo,
        Self::InProgress,
        Self::Done,
        Self::Cancelled,
    ];

    /// The value written in task files.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Todo => "todo",
            Self::InProgress => "in_progress",
            Self::Done => "done",
            Self::Cancelled => "cancelled",
        }
    }

    /// The name shown to people.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Inbox => "Inbox",
            Self::Todo => "To do",
            Self::InProgress => "In progress",
            Self::Done => "Done",
            Self::Cancelled => "Cancelled",
        }
    }

    #[must_use]
    pub const fn is_open(self) -> bool {
        matches!(self, Self::Inbox | Self::Todo | Self::InProgress)
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Status {
    type Err = UnknownValue;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|status| status.as_str() == s)
            .ok_or_else(|| UnknownValue::new("status", s, Self::ALL.map(Self::as_str)))
    }
}

/// How much a task matters, from `Low` to `Urgent`; they compare in that order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    Low,
    Normal,
    High,
    Urgent,
}

impl Priority {
    pub const ALL: [Self; 4] = [Self::Low, Self::Normal, Self::High, Self::Urgent];

    /// The value written in task files.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Normal => "normal",
            Self::High => "high",
            Self::Urgent => "urgent",
        }
    }

    /// The name shown to people.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Normal => "Normal",
            Self::High => "High",
            Self::Urgent => "Urgent",
        }
    }
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Priority {
    type Err = UnknownValue;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|priority| priority.as_str() == s)
            .ok_or_else(|| UnknownValue::new("priority", s, Self::ALL.map(Self::as_str)))
    }
}

/// A value that is not one of a field's allowed values.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("“{value}” is not a {field}; it should be one of {}", expected.join(", "))]
pub struct UnknownValue {
    pub field: &'static str,
    pub value: String,
    pub expected: Vec<&'static str>,
}

impl UnknownValue {
    fn new<const N: usize>(field: &'static str, value: &str, expected: [&'static str; N]) -> Self {
        Self {
            field,
            value: value.to_owned(),
            expected: expected.to_vec(),
        }
    }
}

/// A task's title: text with something in it besides spaces, stored trimmed.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Title(String);

impl Title {
    /// # Errors
    ///
    /// When the title is empty or only spaces.
    pub fn new(title: &str) -> Result<Self, EmptyText> {
        let title = title.trim();
        if title.is_empty() {
            return Err(EmptyText("title"));
        }
        Ok(Self(title.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Title {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A task's category, shown and compared exactly as typed (after trimming). `/` has no special
/// meaning.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Category(String);

impl Category {
    /// # Errors
    ///
    /// When the category is empty or only spaces.
    pub fn new(category: &str) -> Result<Self, EmptyText> {
        let category = category.trim();
        if category.is_empty() {
            return Err(EmptyText("category"));
        }
        Ok(Self(category.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A tag, stored as typed but trimmed and without a leading `#`. Tags compare ignoring case, so
/// `Writing` and `#writing` are the same tag.
#[derive(Clone, Debug)]
pub struct Tag(String);

impl Tag {
    /// # Errors
    ///
    /// When nothing is left after trimming spaces and leading `#`s.
    pub fn new(tag: &str) -> Result<Self, EmptyText> {
        let tag = tag.trim().trim_start_matches('#').trim();
        if tag.is_empty() {
            return Err(EmptyText("tag"));
        }
        Ok(Self(tag.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The form tags are compared by.
    #[must_use]
    pub fn key(&self) -> String {
        self.0.to_lowercase()
    }
}

impl PartialEq for Tag {
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
    }
}

impl Eq for Tag {}

impl Hash for Tag {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key().hash(state);
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Text that must not be empty but is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the {0} is empty")]
pub struct EmptyText(pub &'static str);

/// One task: its fields and its notes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
    pub id: TaskId,
    pub title: Title,
    pub status: Status,
    pub priority: Priority,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    /// Set while the task is closed, to when it was closed.
    pub completed_at: Option<Timestamp>,
    /// A date in the person's local time zone.
    pub due_date: Option<Date>,
    pub parent_id: Option<TaskId>,
    /// The task's prerequisites: tasks that must be done before this one.
    pub depends_on: Vec<TaskId>,
    pub category: Option<Category>,
    tags: Vec<Tag>,
    /// Everything after the front matter, kept byte for byte.
    pub body: String,
}

impl Task {
    /// A task captured into the inbox: status `inbox`, priority `normal`, nothing else set.
    #[must_use]
    pub fn capture(id: TaskId, title: Title, now: Timestamp) -> Self {
        Self {
            id,
            title,
            status: Status::Inbox,
            priority: Priority::Normal,
            created_at: now,
            updated_at: now,
            completed_at: None,
            due_date: None,
            parent_id: None,
            depends_on: Vec::new(),
            category: None,
            tags: Vec::new(),
            body: String::new(),
        }
    }

    #[must_use]
    pub fn is_open(&self) -> bool {
        self.status.is_open()
    }

    /// Changes the status, keeping `completed_at` in step: set when the task closes, cleared
    /// when it opens again.
    pub fn set_status(&mut self, status: Status, now: Timestamp) {
        if status == self.status {
            return;
        }
        self.completed_at = if status.is_open() { None } else { Some(now) };
        self.status = status;
        self.updated_at = now;
    }

    /// Marks the task done.
    pub fn complete(&mut self, now: Timestamp) {
        self.set_status(Status::Done, now);
    }

    /// Opens a closed task again, as `todo`.
    pub fn reopen(&mut self, now: Timestamp) {
        if !self.is_open() {
            self.set_status(Status::Todo, now);
        }
    }

    /// Records that the task changed.
    pub fn touch(&mut self, now: Timestamp) {
        self.updated_at = now;
    }

    #[must_use]
    pub fn tags(&self) -> &[Tag] {
        &self.tags
    }

    /// Replaces the tags, dropping repeats (ignoring case) and keeping the first spelling.
    pub fn set_tags(&mut self, tags: impl IntoIterator<Item = Tag>) {
        self.tags.clear();
        for tag in tags {
            if !self.tags.contains(&tag) {
                self.tags.push(tag);
            }
        }
    }

    #[must_use]
    pub fn has_tag(&self, tag: &Tag) -> bool {
        self.tags.contains(tag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: i64) -> Timestamp {
        Timestamp::from_second(seconds).unwrap()
    }

    fn task() -> Task {
        Task::capture(
            "01JA4XKZ7Q3M8N2P5R4XK9Q2MB".parse().unwrap(),
            Title::new("Prepare project proposal").unwrap(),
            at(1000),
        )
    }

    #[test]
    fn statuses_round_trip_through_their_file_values() {
        for status in Status::ALL {
            assert_eq!(status.as_str().parse::<Status>(), Ok(status));
        }
        for priority in Priority::ALL {
            assert_eq!(priority.as_str().parse::<Priority>(), Ok(priority));
        }
    }

    #[test]
    fn unknown_values_say_what_is_allowed() {
        let error = "later".parse::<Status>().unwrap_err();
        assert_eq!(
            error.to_string(),
            "“later” is not a status; it should be one of inbox, todo, in_progress, done, cancelled"
        );
        assert!(
            "Normal".parse::<Priority>().is_err(),
            "file values are lowercase"
        );
    }

    #[test]
    fn open_and_closed_statuses() {
        assert!(Status::Inbox.is_open());
        assert!(Status::Todo.is_open());
        assert!(Status::InProgress.is_open());
        assert!(!Status::Done.is_open());
        assert!(!Status::Cancelled.is_open());
    }

    #[test]
    fn priorities_compare_from_low_to_urgent() {
        assert!(Priority::Low < Priority::Normal);
        assert!(Priority::High < Priority::Urgent);
    }

    #[test]
    fn titles_are_trimmed_and_must_not_be_empty() {
        assert_eq!(
            Title::new("  Call the bank ").unwrap().as_str(),
            "Call the bank"
        );
        assert_eq!(Title::new("   "), Err(EmptyText("title")));
    }

    #[test]
    fn categories_are_trimmed_and_compared_exactly() {
        assert_eq!(Category::new(" Work ").unwrap().as_str(), "Work");
        assert_ne!(Category::new("Work"), Category::new("work"));
        assert!(Category::new("").is_err());
    }

    #[test]
    fn tags_drop_the_hash_and_compare_ignoring_case() {
        let tag = Tag::new(" #Writing ").unwrap();
        assert_eq!(tag.as_str(), "Writing");
        assert_eq!(tag, Tag::new("writing").unwrap());
        assert!(Tag::new("#").is_err());
        assert!(Tag::new(" ").is_err());
    }

    #[test]
    fn set_tags_drops_repeats_keeping_the_first_spelling() {
        let mut task = task();
        task.set_tags(["Q4", "writing", "q4", "#Writing"].map(|t| Tag::new(t).unwrap()));
        let tags: Vec<_> = task.tags().iter().map(Tag::as_str).collect();
        assert_eq!(tags, ["Q4", "writing"]);
        assert!(task.has_tag(&Tag::new("WRITING").unwrap()));
    }

    #[test]
    fn captured_tasks_land_in_the_inbox() {
        let task = task();
        assert_eq!(task.status, Status::Inbox);
        assert_eq!(task.priority, Priority::Normal);
        assert_eq!(task.created_at, task.updated_at);
        assert_eq!(task.body, "");
        assert!(task.completed_at.is_none());
    }

    #[test]
    fn completing_sets_completed_at_and_reopening_clears_it() {
        let mut task = task();
        task.complete(at(2000));
        assert_eq!(task.status, Status::Done);
        assert_eq!(task.completed_at, Some(at(2000)));
        assert_eq!(task.updated_at, at(2000));

        task.reopen(at(3000));
        assert_eq!(task.status, Status::Todo);
        assert_eq!(task.completed_at, None);
        assert_eq!(task.updated_at, at(3000));
    }

    #[test]
    fn cancelling_closes_the_task() {
        let mut task = task();
        task.set_status(Status::Cancelled, at(2000));
        assert!(!task.is_open());
        assert_eq!(task.completed_at, Some(at(2000)));
    }

    #[test]
    fn setting_the_same_status_changes_nothing() {
        let mut task = task();
        task.complete(at(2000));
        task.complete(at(3000));
        assert_eq!(task.completed_at, Some(at(2000)));
        assert_eq!(task.updated_at, at(2000));
    }

    #[test]
    fn reopening_an_open_task_changes_nothing() {
        let mut task = task();
        task.reopen(at(2000));
        assert_eq!(task.status, Status::Inbox);
        assert_eq!(task.updated_at, at(1000));
    }
}
