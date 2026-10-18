//! Reading and writing a task file: YAML front matter with the task's fields, then its notes.

use std::fmt::Write as _;

use jiff::Timestamp;
use jiff::civil::Date;
use witdo_core::{Category, Priority, Status, Tag, Task, TaskId, Title};
use yaml_rust2::yaml::Hash;
use yaml_rust2::{Yaml, YamlEmitter, YamlLoader};

/// The fields witdo knows, in the order it writes them.
const KNOWN_KEYS: [&str; 12] = [
    "id",
    "title",
    "status",
    "priority",
    "created_at",
    "updated_at",
    "completed_at",
    "due_date",
    "parent_id",
    "depends_on",
    "category",
    "tags",
];

/// What a task file holds besides the task, kept so that writing the task back changes only what
/// was edited: front-matter fields witdo does not know, in their order, and the line break
/// between the front matter and the notes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Extras {
    unknown: Vec<(Yaml, Yaml)>,
    /// `None` for a task witdo creates, or a file that ends with its front matter; then
    /// notes, once there are any, start after a blank line.
    separator: Option<String>,
}

impl Extras {
    /// Settles the line break before the notes once a task is written with some, so that
    /// later writes match what reading the file back would give.
    pub(crate) fn written(&mut self, task: &Task) {
        if self.separator.is_none() && !task.body.is_empty() {
            self.separator = Some("\n".to_owned());
        }
    }
}

/// Why a file could not be read as a task.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{}{message}", line.map(|line| format!("Line {line}: ")).unwrap_or_default())]
pub struct ParseError {
    /// The line in the file, counting from 1, when it is known.
    pub line: Option<usize>,
    pub message: String,
}

/// Reads a task file.
///
/// # Errors
///
/// When the file has no front matter, the YAML does not parse, or a field is missing or has a
/// value witdo does not understand.
pub fn parse(text: &str) -> Result<(Task, Extras), ParseError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let FrontMatter { yaml, rest } = split(text)?;
    let separator = if rest.starts_with("\r\n") {
        "\r\n"
    } else if rest.starts_with('\n') {
        "\n"
    } else {
        ""
    };
    let body = &rest[separator.len()..];

    let documents = YamlLoader::load_from_str(yaml).map_err(|error| ParseError {
        // The scanner counts lines from 1 within the YAML, which starts on the file's second.
        line: Some(error.marker().line() + 1),
        message: format!("The front matter is not valid YAML: {}.", error.info()),
    })?;
    let fields = match documents.into_iter().next() {
        Some(Yaml::Hash(fields)) => fields,
        None | Some(Yaml::Null) => Hash::new(),
        Some(_) => {
            return Err(ParseError {
                line: Some(2),
                message: "The front matter should be a list of fields like `title: …`.".into(),
            });
        }
    };

    let fields = Fields { yaml, fields };
    let mut task = Task::capture(
        fields.required("id", parse_id)?,
        fields.required("title", |value| {
            Title::new(value).map_err(|_| "The title is empty.".into())
        })?,
        fields.required("created_at", parse_timestamp)?,
    );
    task.status = fields.required("status", |value| {
        value.parse::<Status>().map_err(|e| sentence(&e))
    })?;
    task.priority = fields.required("priority", |value| {
        value.parse::<Priority>().map_err(|e| sentence(&e))
    })?;
    task.updated_at = fields.required("updated_at", parse_timestamp)?;
    task.completed_at = fields.optional("completed_at", parse_timestamp)?;
    task.due_date = fields.optional("due_date", parse_date)?;
    task.parent_id = fields.optional("parent_id", parse_id)?;
    task.depends_on = fields.list("depends_on", parse_id)?;
    task.category = fields.optional("category", |value| {
        Category::new(value).map_err(|_| "The category is empty.".into())
    })?;
    task.set_tags(fields.list("tags", |value| {
        Tag::new(value).map_err(|_| "A tag is empty.".into())
    })?);
    body.clone_into(&mut task.body);

    let unknown = fields
        .fields
        .into_iter()
        .filter(|(key, _)| !key.as_str().is_some_and(|key| KNOWN_KEYS.contains(&key)))
        .collect();
    Ok((
        task,
        Extras {
            unknown,
            // With nothing after the front matter there is no line break to keep yet.
            separator: (!rest.is_empty()).then(|| separator.to_owned()),
        },
    ))
}

/// A field witdo does not know could not be written back.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("The field {key} could not be written back: {message}.")]
pub struct SerializeError {
    pub key: String,
    pub message: String,
}

/// Writes a task file: the known fields in their canonical order, then the unknown ones, then
/// the notes exactly as they are.
///
/// # Errors
///
/// When a field witdo does not know cannot be written back as YAML; nothing is written then,
/// rather than losing the field.
pub fn serialize(task: &Task, extras: &Extras) -> Result<String, SerializeError> {
    let mut out = String::from("---\n");
    let mut field = |key: &str, value: String| {
        let _ = writeln!(out, "{key}: {value}");
    };
    field("id", quote(&task.id.to_string()));
    field("title", quote(task.title.as_str()));
    field("status", quote(task.status.as_str()));
    field("priority", quote(task.priority.as_str()));
    field("created_at", quote(&format_timestamp(task.created_at)));
    field("updated_at", quote(&format_timestamp(task.updated_at)));
    field(
        "completed_at",
        or_null(task.completed_at.map(format_timestamp)),
    );
    field(
        "due_date",
        or_null(task.due_date.map(|date| date.to_string())),
    );
    field(
        "parent_id",
        or_null(task.parent_id.map(|id| id.to_string())),
    );
    field(
        "depends_on",
        list(task.depends_on.iter().map(ToString::to_string)),
    );
    field(
        "category",
        or_null(task.category.as_ref().map(|c| c.as_str().to_owned())),
    );
    field(
        "tags",
        list(task.tags().iter().map(|tag| tag.as_str().to_owned())),
    );
    for (key, value) in &extras.unknown {
        out.push_str(&emit_field(key, value)?);
    }
    out.push_str("---\n");
    let separator = match &extras.separator {
        Some(separator) => separator.as_str(),
        None if task.body.is_empty() => "",
        None => "\n",
    };
    out.push_str(separator);
    out.push_str(&task.body);
    Ok(out)
}

struct FrontMatter<'a> {
    yaml: &'a str,
    rest: &'a str,
}

fn split(text: &str) -> Result<FrontMatter<'_>, ParseError> {
    let mut lines = text.split_inclusive('\n');
    let opening = lines.next().unwrap_or_default();
    if opening.trim_end() != "---" {
        return Err(ParseError {
            line: Some(1),
            message: "The file doesn’t start with front matter (a line with just ---).".into(),
        });
    }
    let start = opening.len();
    let mut end = start;
    for line in lines {
        let trimmed = line.trim_end();
        if trimmed == "---" || trimmed == "..." {
            return Ok(FrontMatter {
                yaml: &text[start..end],
                rest: &text[end + line.len()..],
            });
        }
        end += line.len();
    }
    Err(ParseError {
        line: None,
        message: "The front matter never ends: add a line with just --- after the fields.".into(),
    })
}

struct Fields<'a> {
    yaml: &'a str,
    fields: Hash,
}

impl Fields<'_> {
    fn get(&self, key: &str) -> Option<&Yaml> {
        self.fields
            .get(&Yaml::String(key.to_owned()))
            .filter(|value| !matches!(value, Yaml::Null))
    }

    fn required<T>(
        &self,
        key: &str,
        parse: impl Fn(&str) -> Result<T, String>,
    ) -> Result<T, ParseError> {
        match self.get(key) {
            Some(value) => self.scalar(key, value, parse),
            None => Err(ParseError {
                line: None,
                message: format!("The required field `{key}` is missing."),
            }),
        }
    }

    fn optional<T>(
        &self,
        key: &str,
        parse: impl Fn(&str) -> Result<T, String>,
    ) -> Result<Option<T>, ParseError> {
        self.get(key)
            .map(|value| self.scalar(key, value, parse))
            .transpose()
    }

    fn list<T>(
        &self,
        key: &str,
        parse: impl Fn(&str) -> Result<T, String>,
    ) -> Result<Vec<T>, ParseError> {
        match self.get(key) {
            None => Ok(Vec::new()),
            Some(Yaml::Array(items)) => items
                .iter()
                .map(|item| self.scalar(key, item, &parse))
                .collect(),
            Some(_) => Err(self.error(key, format!("`{key}` should be a list, like `{key}: []`."))),
        }
    }

    fn scalar<T>(
        &self,
        key: &str,
        value: &Yaml,
        parse: impl Fn(&str) -> Result<T, String>,
    ) -> Result<T, ParseError> {
        let text = match value {
            Yaml::String(text) | Yaml::Real(text) => text.clone(),
            Yaml::Integer(number) => number.to_string(),
            Yaml::Boolean(flag) => flag.to_string(),
            _ => {
                return Err(self.error(
                    key,
                    format!("`{key}` should be a single value, not a list or fields."),
                ));
            }
        };
        parse(&text).map_err(|message| self.error(key, message))
    }

    /// An error about a field, on the line where the field starts when it can be found.
    fn error(&self, key: &str, message: String) -> ParseError {
        let line = self.yaml.lines().position(|line| {
            line.strip_prefix(key)
                .is_some_and(|rest| rest.trim_start().starts_with(':'))
        });
        ParseError {
            line: line.map(|index| index + 2),
            message,
        }
    }
}

fn parse_id(value: &str) -> Result<TaskId, String> {
    value.parse::<TaskId>().map_err(|error| sentence(&error))
}

fn parse_timestamp(value: &str) -> Result<Timestamp, String> {
    value
        .parse::<Timestamp>()
        .map_err(|_| format!("“{value}” should be a date and time like 2026-10-18T10:00:00Z."))
}

fn parse_date(value: &str) -> Result<Date, String> {
    value
        .parse::<Date>()
        .map_err(|_| format!("“{value}” should be a date like 2026-10-25."))
}

/// Whole seconds in UTC, as in `2026-10-18T10:00:00Z`.
fn format_timestamp(timestamp: Timestamp) -> String {
    Timestamp::from_second(timestamp.as_second())
        .unwrap_or(timestamp)
        .to_string()
}

fn sentence(error: &impl std::fmt::Display) -> String {
    let text = error.to_string();
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        format!("{}{}.", first.to_uppercase(), chars.as_str())
    })
}

/// A YAML double-quoted string.
fn quote(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for c in text.chars() {
        match c {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\t' => quoted.push_str("\\t"),
            '\r' => quoted.push_str("\\r"),
            c if c.is_control() => {
                let _ = write!(quoted, "\\u{:04X}", u32::from(c));
            }
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

fn or_null(value: Option<String>) -> String {
    value.map_or_else(|| "null".to_owned(), |value| quote(&value))
}

fn list(items: impl Iterator<Item = String>) -> String {
    let items: Vec<String> = items.map(|item| quote(&item)).collect();
    format!("[{}]", items.join(", "))
}

/// One unknown field as YAML lines, ending with a line break.
fn emit_field(key: &Yaml, value: &Yaml) -> Result<String, SerializeError> {
    let mut field = Hash::new();
    field.insert(key.clone(), value.clone());
    let mut yaml = String::new();
    let mut emitter = YamlEmitter::new(&mut yaml);
    emitter.multiline_strings(true);
    emitter
        .dump(&Yaml::Hash(field))
        .map_err(|error| SerializeError {
            key: key
                .as_str()
                .map_or_else(|| format!("{key:?}"), |key| format!("`{key}`")),
            message: error.to_string(),
        })?;
    let mut lines = yaml
        .strip_prefix("---\n")
        .or_else(|| yaml.strip_prefix("--- "))
        .unwrap_or(&yaml)
        .to_owned();
    lines.push('\n');
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;

    use super::*;

    const EXAMPLE: &str = r#"---
id: "01JA4XKZ7Q3M8N2P5R4XK9Q2MB"
title: "Prepare project proposal"
status: "todo"
priority: "normal"
created_at: "2026-10-18T10:00:00Z"
updated_at: "2026-10-18T10:00:00Z"
completed_at: null
due_date: "2026-10-25"
parent_id: null
depends_on: []
category: "Work"
tags: ["writing", "q4"]
---

Draft the outline first. Ask Maria for last year's numbers.
"#;

    fn parsed(text: &str) -> Task {
        parse(text).unwrap().0
    }

    fn error(text: &str) -> ParseError {
        parse(text).unwrap_err()
    }

    /// The example with one line replaced (or removed, when `with` is empty).
    fn example_with(line: &str, with: &str) -> String {
        let replaced: Vec<&str> = EXAMPLE
            .lines()
            .map(|l| if l.starts_with(line) { with } else { l })
            .filter(|l| !l.is_empty() || !with.is_empty())
            .collect();
        replaced.join("\n") + "\n"
    }

    #[test]
    fn reads_every_field() {
        let task = parsed(EXAMPLE);
        assert_eq!(task.id.to_string(), "01JA4XKZ7Q3M8N2P5R4XK9Q2MB");
        assert_eq!(task.title.as_str(), "Prepare project proposal");
        assert_eq!(task.status, Status::Todo);
        assert_eq!(task.priority, Priority::Normal);
        assert_eq!(task.created_at, "2026-10-18T10:00:00Z".parse().unwrap());
        assert_eq!(task.completed_at, None);
        assert_eq!(task.due_date, Some(date(2026, 10, 25)));
        assert_eq!(task.parent_id, None);
        assert_eq!(task.depends_on, []);
        assert_eq!(task.category.as_ref().unwrap().as_str(), "Work");
        let tags: Vec<&str> = task.tags().iter().map(Tag::as_str).collect();
        assert_eq!(tags, ["writing", "q4"]);
        assert_eq!(
            task.body,
            "Draft the outline first. Ask Maria for last year's numbers.\n"
        );
    }

    #[test]
    fn writes_the_canonical_form_back_unchanged() {
        let (task, extras) = parse(EXAMPLE).unwrap();
        assert_eq!(serialize(&task, &extras).unwrap(), EXAMPLE);
    }

    #[test]
    fn accepts_plain_yaml_values() {
        let text = "---\nid: 01JA4XKZ7Q3M8N2P5R4XK9Q2MB\ntitle: 2026\nstatus: done\npriority: high\n\
            created_at: 2026-10-18T10:00:00+02:00\nupdated_at: 2026-10-18T10:00:00Z\n\
            completed_at: 2026-10-19T08:30:00Z\ndue_date: 2026-10-25\n\
            depends_on:\n  - 01JA4XKZ7Q3M8N2P5R4XK9Q2MC\ntags:\n  - '#home'\n---\n";
        let task = parsed(text);
        assert_eq!(task.title.as_str(), "2026");
        assert_eq!(task.status, Status::Done);
        assert_eq!(task.created_at, "2026-10-18T08:00:00Z".parse().unwrap());
        assert_eq!(task.depends_on.len(), 1);
        assert_eq!(task.tags()[0].as_str(), "home");
    }

    #[test]
    fn optional_fields_can_be_left_out() {
        let text = "---\nid: \"01JA4XKZ7Q3M8N2P5R4XK9Q2MB\"\ntitle: \"Call the bank\"\nstatus: \"inbox\"\n\
            priority: \"normal\"\ncreated_at: \"2026-10-18T10:00:00Z\"\nupdated_at: \"2026-10-18T10:00:00Z\"\n---\n";
        let task = parsed(text);
        assert_eq!(task.due_date, None);
        assert_eq!(task.tags(), []);
        assert_eq!(task.body, "");
    }

    #[test]
    fn keeps_the_notes_byte_for_byte() {
        for body in [
            "",
            "\n",
            "Notes",
            "\n\n  indented\r\nline\t\n",
            "---\nnot front matter\n",
        ] {
            let text = EXAMPLE.replace(
                "\nDraft the outline first. Ask Maria for last year's numbers.\n",
                body,
            );
            let (task, extras) = parse(&text).unwrap();
            assert_eq!(serialize(&task, &extras).unwrap(), text, "body {body:?}");
        }
    }

    #[test]
    fn notes_without_a_blank_line_stay_that_way() {
        let text = EXAMPLE.replace("---\n\nDraft", "---\nDraft");
        let (task, extras) = parse(&text).unwrap();
        assert_eq!(
            task.body,
            "Draft the outline first. Ask Maria for last year's numbers.\n"
        );
        assert_eq!(serialize(&task, &extras).unwrap(), text);
    }

    #[test]
    fn notes_added_to_a_file_without_any_start_after_a_blank_line() {
        let text = EXAMPLE.replace(
            "\nDraft the outline first. Ask Maria for last year's numbers.\n",
            "",
        );
        let (mut task, mut extras) = parse(&text).unwrap();
        assert_eq!(serialize(&task, &extras).unwrap(), text);
        task.body = "Notes\n".into();
        let written = serialize(&task, &extras).unwrap();
        assert!(written.ends_with("---\n\nNotes\n"), "{written}");
        extras.written(&task);
        assert_eq!(parse(&written).unwrap().1, extras);
    }

    #[test]
    fn new_tasks_put_a_blank_line_before_their_notes() {
        let (mut task, _) = parse(EXAMPLE).unwrap();
        task.body = "Notes\n".into();
        assert!(
            serialize(&task, &Extras::default())
                .unwrap()
                .ends_with("---\n\nNotes\n")
        );
        task.body = String::new();
        assert!(
            serialize(&task, &Extras::default())
                .unwrap()
                .ends_with("tags: [\"writing\", \"q4\"]\n---\n")
        );
    }

    #[test]
    fn keeps_unknown_fields_in_order_after_the_known_ones() {
        let text = EXAMPLE.replace(
            "status: \"todo\"\n",
            "status: \"todo\"\naliases: [\"proposal\"]\nsource:\n  kind: email\n  from: Maria\nenergy: 3\n",
        );
        let (task, extras) = parse(&text).unwrap();
        let written = serialize(&task, &extras).unwrap();
        let tail = written
            .split("tags: [\"writing\", \"q4\"]\n")
            .nth(1)
            .unwrap();
        assert!(
            tail.starts_with(
                "aliases:\n  - proposal\nsource:\n  kind: email\n  from: Maria\nenergy: 3\n---\n"
            ),
            "{tail}"
        );
        assert_eq!(
            parse(&written).unwrap(),
            (task, extras),
            "and reads back the same"
        );
    }

    #[test]
    fn round_trips_awkward_text() {
        let (mut task, extras) = parse(EXAMPLE).unwrap();
        task.title = Title::new("Say \"hi\" \\ wave: #1 — ünïcödé\u{7}").unwrap();
        task.set_tags([Tag::new("a, b").unwrap(), Tag::new("[x]").unwrap()]);
        task.category = Some(Category::new("null").unwrap());
        let (read, _) = parse(&serialize(&task, &extras).unwrap()).unwrap();
        assert_eq!(read, task);
    }

    #[test]
    fn writes_whole_seconds() {
        let (mut task, extras) = parse(EXAMPLE).unwrap();
        task.updated_at = "2026-10-18T10:00:00.987654Z".parse().unwrap();
        assert!(
            serialize(&task, &extras)
                .unwrap()
                .contains("updated_at: \"2026-10-18T10:00:00Z\"\n")
        );
    }

    #[test]
    fn ignores_a_byte_order_mark() {
        assert_eq!(parsed(&format!("\u{feff}{EXAMPLE}")), parsed(EXAMPLE));
    }

    #[test]
    fn explains_files_without_front_matter() {
        assert_eq!(error("# Just notes\n").line, Some(1));
        assert_eq!(error("").line, Some(1));
        let unclosed = error("---\ntitle: x\n");
        assert_eq!(unclosed.line, None);
        assert!(unclosed.message.contains("never ends"));
    }

    #[test]
    fn explains_invalid_yaml_with_its_line() {
        let error = error(&example_with("priority:", "priority: [normal"));
        assert!(
            error
                .message
                .starts_with("The front matter is not valid YAML"),
            "{error}"
        );
        assert!(error.line.is_some());
    }

    #[test]
    fn explains_missing_required_fields() {
        let error = error(&example_with("title:", ""));
        assert_eq!(error.message, "The required field `title` is missing.");
    }

    #[test]
    fn explains_bad_values_on_their_line() {
        let status = error(&example_with("status:", "status: \"later\""));
        assert_eq!(status.line, Some(4));
        assert_eq!(
            status.to_string(),
            "Line 4: “later” is not a status; it should be one of inbox, todo, in_progress, done, cancelled."
        );

        let due = error(&example_with("due_date:", "due_date: \"next week\""));
        assert_eq!(due.line, Some(9));
        assert_eq!(due.message, "“next week” should be a date like 2026-10-25.");

        let id = error(&example_with("id:", "id: \"prepare-proposal\""));
        assert_eq!(id.line, Some(2));

        let title = error(&example_with("title:", "title: \"  \""));
        assert_eq!(title.message, "The title is empty.");

        let tags = error(&example_with("tags:", "tags: \"writing\""));
        assert_eq!(tags.message, "`tags` should be a list, like `tags: []`.");
    }

    #[test]
    fn explains_front_matter_that_is_not_fields() {
        let error = error("---\n- just\n- a list\n---\n");
        assert_eq!(error.line, Some(2));
    }
}
