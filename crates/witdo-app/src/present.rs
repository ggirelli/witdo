//! What the UIs show, worked out once for both: list rows, sidebar counts, a task's detail,
//! and reading what people type into fields.

use jiff::ToSpan;
use jiff::civil::Date;
use witdo_core::{Filter, PrerequisiteState, Priority, Status, Tag, Task, TaskId, View};

use crate::{AppError, Session};

/// How the task list is laid out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layout {
    #[default]
    List,
    /// Subtasks nested under their parents.
    Tree,
}

/// One line of the task list.
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent flags for how a row is drawn"
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: TaskId,
    /// How deep in the tree; always 0 in a list.
    pub depth: usize,
    /// Shown only for context in a tree (an ancestor, or a subtask in another category), not
    /// because it is in the view: draw it dimmed.
    pub context: bool,
    pub title: String,
    pub status: Status,
    pub priority: Priority,
    pub due: Option<Date>,
    /// Open and due before today.
    pub overdue: bool,
    pub category: Option<String>,
    /// Waits for an open prerequisite.
    pub blocked: bool,
    /// A prerequisite was cancelled: worth a look.
    pub prerequisite_cancelled: bool,
    pub has_subtasks: bool,
}

/// How many tasks each built-in view holds, for the sidebar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewCounts {
    pub inbox: usize,
    pub next: usize,
    pub due: usize,
    pub open: usize,
    pub completed: usize,
    pub all: usize,
}

/// Another task, as a detail panel names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Linked {
    pub id: TaskId,
    /// The title, or the ID when the task is missing.
    pub title: String,
    /// `None` when the task is missing.
    pub status: Option<Status>,
}

/// Everything the detail panel shows about one task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Detail {
    pub task: Task,
    pub file_name: String,
    pub parent: Option<Linked>,
    pub subtasks: Vec<Linked>,
    /// "Blocked by": the task's prerequisites and their states.
    pub prerequisites: Vec<(Linked, PrerequisiteState)>,
    /// "Blocks": tasks waiting for this one.
    pub dependents: Vec<Linked>,
    pub blocked: bool,
    pub on_parent_cycle: bool,
    pub on_dependency_cycle: bool,
}

impl Session {
    /// The rows of the task list for a view, filter and layout.
    #[must_use]
    pub fn rows(&self, view: &View, filter: &Filter, layout: Layout) -> Vec<Row> {
        let today = self.clock().today();
        let tasks = self.tasks();
        let row = |id: TaskId, depth: usize, context: bool| {
            let task = tasks.get(id)?;
            Some(Row {
                id,
                depth,
                context,
                title: task.title.to_string(),
                status: task.status,
                priority: task.priority,
                due: task.due_date,
                overdue: task.is_open() && task.due_date.is_some_and(|due| due < today),
                category: task.category.as_ref().map(ToString::to_string),
                blocked: tasks.is_blocked(id),
                prerequisite_cancelled: tasks.has_cancelled_prerequisite(id),
                has_subtasks: !tasks.children(id).is_empty(),
            })
        };
        match layout {
            Layout::List => tasks
                .list(view, filter, today)
                .into_iter()
                .filter_map(|id| row(id, 0, false))
                .collect(),
            Layout::Tree => tasks
                .tree(view, filter, today)
                .into_iter()
                .filter_map(|tree_row| row(tree_row.id, tree_row.depth, !tree_row.matches))
                .collect(),
        }
    }

    #[must_use]
    pub fn view_counts(&self) -> ViewCounts {
        let today = self.clock().today();
        let count = |view: View| self.tasks().list(&view, &Filter::default(), today).len();
        ViewCounts {
            inbox: count(View::Inbox),
            next: count(View::Next),
            due: count(View::Due),
            open: count(View::Open),
            completed: count(View::Completed),
            all: self.tasks().len(),
        }
    }

    /// The detail panel for a task, or `None` if it is not loaded.
    #[must_use]
    pub fn detail(&self, id: TaskId) -> Option<Detail> {
        let tasks = self.tasks();
        let task = tasks.get(id)?.clone();
        let linked = |id: TaskId| {
            tasks.get(id).map_or_else(
                || Linked {
                    id,
                    title: id.to_string(),
                    status: None,
                },
                |task| Linked {
                    id,
                    title: task.title.to_string(),
                    status: Some(task.status),
                },
            )
        };
        Some(Detail {
            file_name: self.file_name(id).unwrap_or_default().to_owned(),
            parent: task.parent_id.map(linked),
            subtasks: tasks.children(id).iter().map(|&id| linked(id)).collect(),
            prerequisites: tasks
                .prerequisites(id)
                .into_iter()
                .map(|(id, state)| (linked(id), state))
                .collect(),
            dependents: tasks.dependents(id).iter().map(|&id| linked(id)).collect(),
            blocked: tasks.is_blocked(id),
            on_parent_cycle: tasks.is_on_parent_cycle(id),
            on_dependency_cycle: tasks.is_on_dependency_cycle(id),
            task,
        })
    }

    /// Tasks whose title contains `text` (ignoring case), for picking a parent or a
    /// prerequisite; open tasks first, then by title.
    #[must_use]
    pub fn find_tasks(&self, text: &str, except: Option<TaskId>) -> Vec<Linked> {
        let text = text.trim().to_lowercase();
        let mut found: Vec<&Task> = self
            .tasks()
            .iter()
            .filter(|task| Some(task.id) != except)
            .filter(|task| task.title.as_str().to_lowercase().contains(&text))
            .collect();
        found.sort_by_key(|task| (!task.is_open(), task.title.as_str().to_lowercase()));
        found
            .into_iter()
            .map(|task| Linked {
                id: task.id,
                title: task.title.to_string(),
                status: Some(task.status),
            })
            .collect()
    }
}

/// Reads a typed due date: empty for none, `today`, `tomorrow`, or `YYYY-MM-DD`.
///
/// # Errors
///
/// When the text is none of those.
pub fn parse_due(text: &str, today: Date) -> Result<Option<Date>, AppError> {
    let text = text.trim();
    match text.to_lowercase().as_str() {
        "" => Ok(None),
        "today" => Ok(Some(today)),
        "tomorrow" => Ok(Some(today.saturating_add(1.day()))),
        _ => text.parse::<Date>().map(Some).map_err(|_| {
            AppError::Input(format!(
                "“{text}” is not a date: use YYYY-MM-DD, “today” or “tomorrow”."
            ))
        }),
    }
}

/// Reads typed tags, separated by commas or spaces, with or without `#`.
#[must_use]
pub fn parse_tags(text: &str) -> Vec<Tag> {
    text.split(|c: char| c == ',' || c.is_whitespace())
        .filter_map(|tag| Tag::new(tag).ok())
        .collect()
}

/// Tags as they are typed back into the field.
#[must_use]
pub fn format_tags(tags: &[Tag]) -> String {
    tags.iter().map(Tag::as_str).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;
    use tempfile::TempDir;
    use witdo_io::workspace::Workspace;
    use witdo_io::write::Overwrite;

    use super::*;
    use crate::Clock;

    const TODAY: Date = date(2026, 10, 21);

    fn session() -> (TempDir, Session) {
        let dir = TempDir::new().unwrap();
        Workspace::init(dir.path(), None).unwrap();
        let clock = Clock::fixed("2026-10-21T12:00:00Z".parse().unwrap());
        let session = Session::open(dir.path(), clock).unwrap();
        (dir, session)
    }

    fn edit(session: &mut Session, id: TaskId, change: impl FnOnce(&mut Task)) {
        let mut draft = session.draft(id).unwrap();
        change(&mut draft.task);
        session.save(draft, Overwrite::IfUnchanged).unwrap();
    }

    #[test]
    fn rows_carry_what_the_list_shows() {
        let (_dir, mut session) = session();
        let first = session.capture("Get the forms").unwrap();
        let second = session.capture("Fill in the forms").unwrap();
        edit(&mut session, second, |task| {
            task.depends_on = vec![first];
            task.due_date = Some(date(2026, 10, 1));
        });
        let rows = session.rows(&View::All, &Filter::default(), Layout::List);
        let row = rows.iter().find(|row| row.id == second).unwrap();
        assert!(row.blocked);
        assert!(row.overdue);
        assert_eq!(row.title, "Fill in the forms");
        assert!(!rows.iter().find(|row| row.id == first).unwrap().blocked);
    }

    #[test]
    fn tree_rows_mark_context() {
        let (_dir, mut session) = session();
        let parent = session.capture("Move house").unwrap();
        let child = session.capture("Pack books").unwrap();
        edit(&mut session, child, |task| {
            task.parent_id = Some(parent);
            task.status = Status::Todo;
        });
        let rows = session.rows(&View::Next, &Filter::default(), Layout::Tree);
        let shape: Vec<(TaskId, usize, bool)> =
            rows.iter().map(|r| (r.id, r.depth, r.context)).collect();
        assert_eq!(shape, [(parent, 0, true), (child, 1, false)]);
        assert!(rows[0].has_subtasks);
    }

    #[test]
    fn counts_each_view() {
        let (_dir, mut session) = session();
        session.capture("A").unwrap();
        let b = session.capture("B").unwrap();
        session.complete(b).unwrap();
        let counts = session.view_counts();
        assert_eq!(
            (counts.inbox, counts.open, counts.completed, counts.all),
            (1, 1, 1, 2)
        );
    }

    #[test]
    fn detail_shows_links_both_ways() {
        let (_dir, mut session) = session();
        let parent = session.capture("Move house").unwrap();
        let first = session.capture("Book the van").unwrap();
        let second = session.capture("Load the van").unwrap();
        edit(&mut session, first, |task| task.parent_id = Some(parent));
        edit(&mut session, second, |task| task.depends_on = vec![first]);

        let detail = session.detail(second).unwrap();
        assert_eq!(detail.prerequisites[0].0.title, "Book the van");
        assert_eq!(detail.prerequisites[0].1, PrerequisiteState::Open);
        assert!(detail.blocked);

        let detail = session.detail(first).unwrap();
        assert_eq!(detail.parent.unwrap().title, "Move house");
        assert_eq!(detail.dependents[0].id, second);
        assert_eq!(session.detail(parent).unwrap().subtasks[0].id, first);
    }

    #[test]
    fn finds_tasks_to_link_to() {
        let (_dir, mut session) = session();
        let van = session.capture("Book the van").unwrap();
        let load = session.capture("Load the VAN").unwrap();
        session.capture("Pack books").unwrap();
        let found: Vec<TaskId> = session
            .find_tasks("van", Some(load))
            .iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(found, [van]);
    }

    #[test]
    fn reads_typed_due_dates() {
        assert_eq!(parse_due("", TODAY).unwrap(), None);
        assert_eq!(parse_due(" Today ", TODAY).unwrap(), Some(TODAY));
        assert_eq!(
            parse_due("tomorrow", TODAY).unwrap(),
            Some(date(2026, 10, 22))
        );
        assert_eq!(
            parse_due("2026-12-01", TODAY).unwrap(),
            Some(date(2026, 12, 1))
        );
        assert!(
            parse_due("next week", TODAY)
                .unwrap_err()
                .to_string()
                .contains("YYYY-MM-DD")
        );
    }

    #[test]
    fn reads_typed_tags() {
        let tags = parse_tags("#writing, q4  home,,");
        assert_eq!(format_tags(&tags), "writing, q4, home");
    }
}
