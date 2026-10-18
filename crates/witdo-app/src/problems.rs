use witdo_core::{Problem, TaskId, TaskSet};
use witdo_io::workspace::{FileProblem, IgnoredReason};

use crate::Session;

/// One entry of the Problems panel: what is wrong, said plainly, and what it is about. witdo
/// never fixes these itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProblemReport {
    pub kind: ProblemKind,
    /// The files involved, in `tasks/`.
    pub files: Vec<String>,
    /// The loaded tasks involved.
    pub tasks: Vec<TaskId>,
    /// The line in the file, when it is known.
    pub line: Option<usize>,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProblemKind {
    /// A file that is not a valid task; it does not load.
    Malformed,
    /// Files sharing an ID; none of them loads.
    DuplicateId,
    Unreadable,
    /// Something in `tasks/` that is not a task file. Only a note.
    Ignored,
    MissingParent,
    MissingPrerequisite,
    ParentCycle,
    DependencyCycle,
}

impl ProblemKind {
    /// Whether the problem keeps something from loading, as opposed to a note or a link
    /// problem on a task that did load.
    #[must_use]
    pub const fn hides_tasks(self) -> bool {
        matches!(self, Self::Malformed | Self::DuplicateId | Self::Unreadable)
    }
}

impl Session {
    /// Everything for the Problems panel: files first, by name, then links between tasks.
    #[must_use]
    pub fn problems(&self) -> Vec<ProblemReport> {
        let files = self.file_problems().iter().map(file_report);
        let links = self
            .tasks()
            .problems()
            .into_iter()
            .map(|problem| self.link_report(&problem));
        files.chain(links).collect()
    }

    fn link_report(&self, problem: &Problem) -> ProblemReport {
        let tasks = self.tasks();
        let report = |kind, involved: Vec<TaskId>, message| ProblemReport {
            kind,
            files: involved
                .iter()
                .filter_map(|&id| self.file_name(id).map(str::to_owned))
                .collect(),
            tasks: involved,
            line: None,
            message,
        };
        match problem {
            Problem::MissingParent { task, parent } => report(
                ProblemKind::MissingParent,
                vec![*task],
                format!(
                    "{} is a subtask of a task that doesn’t exist (ID {parent}), so it is shown \
                     at the top level.",
                    quoted(tasks, *task)
                ),
            ),
            Problem::MissingPrerequisite { task, prerequisite } => report(
                ProblemKind::MissingPrerequisite,
                vec![*task],
                format!(
                    "{} waits for a task that doesn’t exist (ID {prerequisite}).",
                    quoted(tasks, *task)
                ),
            ),
            Problem::ParentCycle { tasks: cycle } if cycle.len() == 1 => report(
                ProblemKind::ParentCycle,
                cycle.clone(),
                format!(
                    "{} is its own parent, so it is shown at the top level.",
                    quoted(tasks, cycle[0])
                ),
            ),
            Problem::ParentCycle { tasks: cycle } => report(
                ProblemKind::ParentCycle,
                cycle.clone(),
                format!(
                    "{} are each other’s parents, which makes a loop, so they are shown at the \
                     top level.",
                    listed(tasks, cycle)
                ),
            ),
            Problem::DependencyCycle { tasks: cycle } if cycle.len() == 1 => report(
                ProblemKind::DependencyCycle,
                cycle.clone(),
                format!("{} waits for itself.", quoted(tasks, cycle[0])),
            ),
            Problem::DependencyCycle { tasks: cycle } => report(
                ProblemKind::DependencyCycle,
                cycle.clone(),
                format!(
                    "{} wait for each other, so none of them can start.",
                    listed(tasks, cycle)
                ),
            ),
        }
    }
}

fn file_report(problem: &FileProblem) -> ProblemReport {
    let report = |kind, files: Vec<String>, line, message| ProblemReport {
        kind,
        files,
        tasks: Vec::new(),
        line,
        message,
    };
    match problem {
        FileProblem::Malformed { file, error } => report(
            ProblemKind::Malformed,
            vec![file.clone()],
            error.line,
            error.message.clone(),
        ),
        FileProblem::DuplicateId { id, files } => report(
            ProblemKind::DuplicateId,
            files.clone(),
            None,
            format!(
                "{} files have the same ID ({id}), probably because one is a copy. None of them \
                 is shown until only one is left.",
                files.len()
            ),
        ),
        FileProblem::Unreadable { file, message } => report(
            ProblemKind::Unreadable,
            vec![file.clone()],
            None,
            format!("It could not be read: {message}"),
        ),
        FileProblem::Ignored { file, reason } => report(
            ProblemKind::Ignored,
            vec![file.clone()],
            None,
            match reason {
                IgnoredReason::Directory => {
                    "It is a folder; witdo only reads task files directly in tasks/."
                }
                IgnoredReason::NotMarkdown => {
                    "It is not a Markdown (.md) file, so witdo leaves it alone."
                }
                IgnoredReason::NameNotUtf8 => {
                    "Its name is not valid text, so witdo leaves it alone."
                }
            }
            .to_owned(),
        ),
    }
}

/// A task's title in quotes, or its ID if it is not loaded.
fn quoted(tasks: &TaskSet, id: TaskId) -> String {
    tasks
        .get(id)
        .map_or_else(|| id.to_string(), |task| format!("“{}”", task.title))
}

/// Titles in quotes, joined as in “A”, “B” and “C”.
fn listed(tasks: &TaskSet, ids: &[TaskId]) -> String {
    let titles: Vec<String> = ids.iter().map(|&id| quoted(tasks, id)).collect();
    match titles.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => titles.concat(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;
    use witdo_io::workspace::Workspace;
    use witdo_io::write::Overwrite;

    use super::*;
    use crate::Clock;

    fn session() -> (TempDir, Session) {
        let dir = TempDir::new().unwrap();
        Workspace::init(dir.path(), None).unwrap();
        let session = Session::open(dir.path(), Clock::system()).unwrap();
        (dir, session)
    }

    /// Writes a task file by hand, as another tool would, with extra front-matter lines.
    fn put(session: &Session, name: &str, id: &str, title: &str, extra: &str) {
        let text = format!(
            "---\nid: {id}\ntitle: {title}\nstatus: todo\npriority: normal\n\
             created_at: 2026-10-18T10:00:00Z\nupdated_at: 2026-10-18T10:00:00Z\n{extra}---\n"
        );
        fs::write(session.workspace().tasks_dir().join(name), text).unwrap();
    }

    const A: &str = "01JA4XKZ7Q3M8N2P5R4XK9Q2MA";
    const B: &str = "01JA4XKZ7Q3M8N2P5R4XK9Q2MB";
    const C: &str = "01JA4XKZ7Q3M8N2P5R4XK9Q2MC";
    const MISSING: &str = "01JA4XKZ7Q3M8N2P5R4XK9Q2MZ";

    fn messages(session: &Session) -> Vec<String> {
        session.problems().into_iter().map(|p| p.message).collect()
    }

    #[test]
    fn a_clean_workspace_has_no_problems() {
        let (_dir, mut session) = session();
        session.capture("Call the bank").unwrap();
        assert_eq!(session.problems(), []);
    }

    #[test]
    fn reports_files_that_did_not_load() {
        let (_dir, mut session) = session();
        put(&session, "a.md", A, "Alpha", "");
        put(&session, "a copy.md", A, "Alpha", "");
        fs::write(
            session.workspace().tasks_dir().join("b.md"),
            "---\nstatus: maybe\n---\n",
        )
        .unwrap();
        fs::write(session.workspace().tasks_dir().join("c.txt"), "").unwrap();
        session.refresh().unwrap();
        let problems = session.problems();
        assert_eq!(problems[0].kind, ProblemKind::DuplicateId);
        assert_eq!(problems[0].files, ["a copy.md", "a.md"]);
        assert!(problems[0].message.starts_with("2 files have the same ID"));
        assert_eq!(problems[1].kind, ProblemKind::Malformed);
        assert_eq!(problems[1].files, ["b.md"]);
        assert_eq!(problems[2].kind, ProblemKind::Ignored);
        assert!(!problems[2].kind.hides_tasks());
    }

    #[test]
    fn reports_missing_tasks_by_title() {
        let (_dir, mut session) = session();
        put(
            &session,
            "a.md",
            A,
            "Alpha",
            &format!("parent_id: {MISSING}\ndepends_on: [{MISSING}]\n"),
        );
        session.refresh().unwrap();
        assert_eq!(
            messages(&session),
            [
                format!(
                    "“Alpha” is a subtask of a task that doesn’t exist (ID {MISSING}), so it is shown at the top level."
                ),
                format!("“Alpha” waits for a task that doesn’t exist (ID {MISSING})."),
            ]
        );
        assert_eq!(session.problems()[0].files, ["a.md"]);
    }

    #[test]
    fn reports_cycles_by_title() {
        let (_dir, mut session) = session();
        put(
            &session,
            "a.md",
            A,
            "Alpha",
            &format!("parent_id: {B}\ndepends_on: [{C}]\n"),
        );
        put(&session, "b.md", B, "Beta", &format!("parent_id: {A}\n"));
        put(
            &session,
            "c.md",
            C,
            "Gamma",
            &format!("depends_on: [{A}, {C}]\n"),
        );
        session.refresh().unwrap();
        assert_eq!(
            messages(&session),
            [
                "“Alpha” and “Beta” are each other’s parents, which makes a loop, so they are shown at the top level.",
                "“Alpha” and “Gamma” wait for each other, so none of them can start.",
            ]
        );
    }

    #[test]
    fn reports_a_task_that_is_its_own_parent() {
        let (_dir, mut session) = session();
        put(
            &session,
            "a.md",
            A,
            "Alpha",
            &format!("parent_id: {A}\ndepends_on: [{A}]\n"),
        );
        session.refresh().unwrap();
        assert_eq!(
            messages(&session),
            [
                "“Alpha” is its own parent, so it is shown at the top level.",
                "“Alpha” waits for itself."
            ]
        );
    }

    #[test]
    fn problems_go_away_once_fixed() {
        let (_dir, mut session) = session();
        put(
            &session,
            "a.md",
            A,
            "Alpha",
            &format!("depends_on: [{MISSING}]\n"),
        );
        session.refresh().unwrap();
        let id = A.parse().unwrap();
        let mut draft = session.draft(id).unwrap();
        draft.task.depends_on.clear();
        session.save(draft, Overwrite::IfUnchanged).unwrap();
        assert_eq!(session.problems(), []);
    }
}
