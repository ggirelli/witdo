use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;

use witdo_core::{
    CompletionWarnings, DeletionPlan, Status, SubtaskChoice, Task, TaskId, TaskSet, Title,
};
use witdo_io::watch::{WatchEvent, Watcher};
use witdo_io::workspace::{FileProblem, TaskFile, Workspace};
use witdo_io::write::{Overwrite, WriteError};

use crate::{AppError, Clock};

/// One open workspace: its tasks in memory, kept in step with their files.
pub struct Session {
    workspace: Workspace,
    tasks: TaskSet,
    files: HashMap<TaskId, TaskFile>,
    file_problems: Vec<FileProblem>,
    clock: Clock,
}

/// A copy of a task being edited, and the version of the task it started from. Saving it
/// stops with a conflict if the task changed in the meantime, on disk or in witdo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Draft {
    pub task: Task,
    revision: u64,
}

impl Draft {
    #[must_use]
    pub fn id(&self) -> TaskId {
        self.task.id
    }

    /// Which version of the task's file the draft started from; it changes whenever the file
    /// does.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }
}

/// What deleting a task did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Deleted {
    /// Names of the files now in `.witdo/trash/`.
    pub trashed: Vec<String>,
    pub moved_to_top_level: Vec<TaskId>,
    pub unlinked: Vec<TaskId>,
}

impl Session {
    /// Opens a workspace and loads its tasks.
    ///
    /// # Errors
    ///
    /// When the directory is not a workspace or `tasks/` cannot be read.
    pub fn open(root: &Path, clock: Clock) -> Result<Self, AppError> {
        let workspace = Workspace::open(root)?;
        let mut session = Self {
            workspace,
            tasks: TaskSet::default(),
            files: HashMap::new(),
            file_problems: Vec::new(),
            clock,
        };
        session.refresh()?;
        Ok(session)
    }

    #[must_use]
    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    #[must_use]
    pub fn tasks(&self) -> &TaskSet {
        &self.tasks
    }

    #[must_use]
    pub fn clock(&self) -> &Clock {
        &self.clock
    }

    /// Files in `tasks/` that did not load.
    #[must_use]
    pub fn file_problems(&self) -> &[FileProblem] {
        &self.file_problems
    }

    /// The name of a task's file in `tasks/`.
    #[must_use]
    pub fn file_name(&self, id: TaskId) -> Option<&str> {
        self.files.get(&id).map(|file| file.name.as_str())
    }

    /// Reads every task file again, dropping what was in memory. This is also how the person
    /// chooses "Reload" after a conflict.
    ///
    /// # Errors
    ///
    /// When `tasks/` cannot be read; the session keeps what it had.
    pub fn refresh(&mut self) -> Result<(), AppError> {
        let scan = self.workspace.scan()?;
        let mut files = HashMap::with_capacity(scan.tasks.len());
        let tasks = TaskSet::from_tasks(scan.tasks.into_iter().map(|loaded| {
            files.insert(loaded.task.id, loaded.file);
            loaded.task
        }));
        self.tasks = tasks;
        self.files = files;
        self.file_problems = scan.problems;
        Ok(())
    }

    /// Creates a task in the inbox from a title.
    ///
    /// # Errors
    ///
    /// When the title is empty or the file cannot be written.
    pub fn capture(&mut self, title: &str) -> Result<TaskId, AppError> {
        let task = Task::capture(TaskId::generate(), Title::new(title)?, self.clock.now());
        self.tasks.check(&task)?;
        let file = self.workspace.create(&task)?;
        let id = task.id;
        self.tasks.insert(task)?;
        self.files.insert(id, file);
        Ok(id)
    }

    /// A copy of a task to edit and then [save](Self::save).
    #[must_use]
    pub fn draft(&self, id: TaskId) -> Option<Draft> {
        Some(Draft {
            task: self.tasks.get(id)?.clone(),
            revision: self.files.get(&id)?.hash,
        })
    }

    /// Saves an edited task. Nothing happens when nothing changed.
    ///
    /// # Errors
    ///
    /// A conflict when the task changed since the draft was made (unless `overwrite` says to
    /// go ahead); when the edit would add a problem, such as a cycle; or when the file cannot
    /// be written. Nothing is changed then.
    pub fn save(&mut self, draft: Draft, overwrite: Overwrite) -> Result<(), AppError> {
        let id = draft.id();
        let file = self.files.get(&id).ok_or(AppError::UnknownTask(id))?;
        if self.tasks.get(id) == Some(&draft.task) {
            return Ok(());
        }
        if overwrite == Overwrite::IfUnchanged && draft.revision != file.hash {
            return Err(AppError::Conflict {
                file: file.name.clone(),
                deleted: false,
            });
        }
        let mut task = draft.task;
        self.tasks.check(&task)?;
        task.touch(self.clock.now());
        let saved = self.workspace.save(&task, file, overwrite)?;
        self.tasks.insert(task)?;
        self.files.insert(id, saved);
        Ok(())
    }

    /// Open prerequisites and subtasks to warn about before [completing](Self::complete) a
    /// task. Completing goes ahead anyway if the person confirms.
    #[must_use]
    pub fn completion_warnings(&self, id: TaskId) -> CompletionWarnings {
        self.tasks.completion_warnings(id)
    }

    /// Marks a task done. Other tasks are never changed.
    ///
    /// # Errors
    ///
    /// As for [`Session::save`].
    pub fn complete(&mut self, id: TaskId) -> Result<(), AppError> {
        self.set_status(id, Status::Done)
    }

    /// Opens a closed task again, as `todo`.
    ///
    /// # Errors
    ///
    /// As for [`Session::save`].
    pub fn reopen(&mut self, id: TaskId) -> Result<(), AppError> {
        let mut draft = self.draft(id).ok_or(AppError::UnknownTask(id))?;
        draft.task.reopen(self.clock.now());
        self.save(draft, Overwrite::IfUnchanged)
    }

    /// Changes a task's status, keeping `completed_at` in step.
    ///
    /// # Errors
    ///
    /// As for [`Session::save`].
    pub fn set_status(&mut self, id: TaskId, status: Status) -> Result<(), AppError> {
        let mut draft = self.draft(id).ok_or(AppError::UnknownTask(id))?;
        draft.task.set_status(status, self.clock.now());
        self.save(draft, Overwrite::IfUnchanged)
    }

    /// What deleting a task would change, for the confirmation.
    #[must_use]
    pub fn deletion_plan(&self, id: TaskId, subtasks: SubtaskChoice) -> DeletionPlan {
        self.tasks.deletion_plan(id, subtasks)
    }

    /// Deletes a task: moves its file (and its subtasks', if chosen) to the trash, moves its
    /// remaining subtasks to the top level, and removes it from the prerequisites of tasks that
    /// depended on it.
    ///
    /// # Errors
    ///
    /// A conflict when any file involved changed outside witdo (unless `overwrite` says to go
    /// ahead), checked before anything changes; or when a file cannot be written, in which
    /// case the steps before it stay done.
    pub fn delete(
        &mut self,
        id: TaskId,
        subtasks: SubtaskChoice,
        overwrite: Overwrite,
    ) -> Result<Deleted, AppError> {
        let plan = self.tasks.deletion_plan(id, subtasks);
        if plan.delete.is_empty() {
            return Err(AppError::UnknownTask(id));
        }
        if overwrite == Overwrite::IfUnchanged {
            let involved = plan
                .delete
                .iter()
                .chain(&plan.move_to_top_level)
                .chain(&plan.unlink);
            for task in involved {
                if let Some(file) = self.files.get(task) {
                    self.workspace.check_unchanged(file)?;
                }
            }
        }

        let mut deleted = Deleted::default();
        // Trash first: should a later step fail, what is left is a dangling link, which the
        // Problems panel shows, rather than a task that silently lost a link.
        for task in &plan.delete {
            if let Some(file) = self.files.get(task) {
                let trashed = self.workspace.trash(file, overwrite, self.clock.now())?;
                deleted.trashed.extend(trashed);
            }
            self.tasks.remove(*task);
            self.files.remove(task);
        }
        for &task in &plan.move_to_top_level {
            if let Some(mut draft) = self.draft(task) {
                draft.task.parent_id = None;
                self.save(draft, overwrite)?;
                deleted.moved_to_top_level.push(task);
            }
        }
        let gone: HashSet<TaskId> = plan.delete.iter().copied().collect();
        for &task in &plan.unlink {
            if let Some(mut draft) = self.draft(task) {
                draft
                    .task
                    .depends_on
                    .retain(|prerequisite| !gone.contains(prerequisite));
                self.save(draft, overwrite)?;
                deleted.unlinked.push(task);
            }
        }
        Ok(deleted)
    }

    /// Starts watching the workspace for changes made elsewhere; pass what `on_event` receives
    /// to [`Session::apply`] on the thread that owns the session.
    ///
    /// # Errors
    ///
    /// When the operating system refuses to watch the directory.
    pub fn watch(
        &self,
        on_event: impl FnMut(WatchEvent) + Send + 'static,
    ) -> Result<Watcher, AppError> {
        Ok(self.workspace.watch(on_event)?)
    }

    /// Takes in changes the watcher saw. Files witdo wrote itself are recognised by their hash
    /// and ignored; anything else makes the session read the workspace again. Returns whether
    /// it did.
    ///
    /// # Errors
    ///
    /// When the files cannot be read again.
    pub fn apply(&mut self, event: &WatchEvent) -> Result<bool, AppError> {
        let changed = match event {
            WatchEvent::Changed(names) => self.changed_elsewhere(names)?,
            WatchEvent::Error(_) => true,
        };
        if changed {
            self.refresh()?;
        }
        Ok(changed)
    }

    fn changed_elsewhere(&self, names: &BTreeSet<String>) -> Result<bool, AppError> {
        for name in names {
            let known = self
                .files
                .values()
                .find(|file| file.name == *name)
                .map(|file| file.hash);
            let on_disk = self.workspace.file_hash(name)?;
            let had_problem = self.file_problems.iter().any(|problem| match problem {
                FileProblem::DuplicateId { files, .. } => files.contains(name),
                problem => problem.file() == name,
            });
            let changed = match (known, on_disk) {
                (Some(known), Some(on_disk)) => known != on_disk,
                (Some(_), None) | (None, Some(_)) => true,
                // Gone and not loaded: a file witdo renamed or trashed, unless it had a problem.
                (None, None) => had_problem,
            };
            if changed {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

impl From<WriteError> for AppError {
    fn from(error: WriteError) -> Self {
        match error {
            WriteError::ChangedOnDisk { file } => Self::Conflict {
                file,
                deleted: false,
            },
            WriteError::DeletedOnDisk { file } => Self::Conflict {
                file,
                deleted: true,
            },
            error => Self::Write(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use jiff::Timestamp;
    use tempfile::TempDir;
    use witdo_core::{EditError, Priority};

    use super::*;

    fn clock() -> Clock {
        Clock::fixed("2026-10-21T09:00:00Z".parse().unwrap())
    }

    fn session() -> (TempDir, Session) {
        let dir = TempDir::new().unwrap();
        Workspace::init(dir.path(), None).unwrap();
        let session = Session::open(dir.path(), clock()).unwrap();
        (dir, session)
    }

    fn reopened(dir: &TempDir) -> Session {
        Session::open(dir.path(), clock()).unwrap()
    }

    fn path(session: &Session, id: TaskId) -> std::path::PathBuf {
        session
            .workspace()
            .tasks_dir()
            .join(session.file_name(id).unwrap())
    }

    fn edit(
        session: &mut Session,
        id: TaskId,
        change: impl FnOnce(&mut Task),
    ) -> Result<(), AppError> {
        let mut draft = session.draft(id).unwrap();
        change(&mut draft.task);
        session.save(draft, Overwrite::IfUnchanged)
    }

    #[test]
    fn captured_tasks_are_files_that_load_again() {
        let (dir, mut session) = session();
        let id = session.capture("  Call the bank ").unwrap();
        let task = session.tasks().get(id).unwrap().clone();
        assert_eq!(task.title.as_str(), "Call the bank");
        assert_eq!(task.status, Status::Inbox);
        assert_eq!(
            task.created_at,
            "2026-10-21T09:00:00Z".parse::<Timestamp>().unwrap()
        );
        assert!(
            session
                .file_name(id)
                .unwrap()
                .starts_with("call-the-bank--")
        );

        let again = reopened(&dir);
        assert_eq!(again.tasks().get(id), Some(&task));
    }

    #[test]
    fn capture_refuses_an_empty_title() {
        let (_dir, mut session) = session();
        assert!(matches!(session.capture("   "), Err(AppError::Title(_))));
        assert!(session.tasks().is_empty());
    }

    #[test]
    fn saving_persists_the_edit_and_the_update_time() {
        let (dir, mut session) = session();
        let id = session.capture("Call the bank").unwrap();
        session.clock = Clock::fixed("2026-10-22T09:00:00Z".parse().unwrap());
        edit(&mut session, id, |task| task.priority = Priority::High).unwrap();
        let task = reopened(&dir).tasks().get(id).unwrap().clone();
        assert_eq!(task.priority, Priority::High);
        assert_eq!(
            task.updated_at,
            "2026-10-22T09:00:00Z".parse::<Timestamp>().unwrap()
        );
    }

    #[test]
    fn saving_an_unchanged_draft_writes_nothing() {
        let (_dir, mut session) = session();
        let id = session.capture("Call the bank").unwrap();
        let before = fs::read_to_string(path(&session, id)).unwrap();
        session.clock = Clock::fixed("2026-10-22T09:00:00Z".parse().unwrap());
        let draft = session.draft(id).unwrap();
        session.save(draft, Overwrite::IfUnchanged).unwrap();
        assert_eq!(fs::read_to_string(path(&session, id)).unwrap(), before);
    }

    #[test]
    fn saving_refuses_edits_that_add_problems() {
        let (_dir, mut session) = session();
        let a = session.capture("A").unwrap();
        let b = session.capture("B").unwrap();
        edit(&mut session, b, |task| task.parent_id = Some(a)).unwrap();
        let error = edit(&mut session, a, |task| task.parent_id = Some(b)).unwrap_err();
        assert!(matches!(error, AppError::Edit(EditError::ParentCycle)));
        assert_eq!(session.tasks().get(a).unwrap().parent_id, None);
    }

    #[test]
    fn complete_and_reopen_persist() {
        let (dir, mut session) = session();
        let id = session.capture("Call the bank").unwrap();
        session.complete(id).unwrap();
        let done = reopened(&dir).tasks().get(id).unwrap().clone();
        assert_eq!(done.status, Status::Done);
        assert!(done.completed_at.is_some());

        session.reopen(id).unwrap();
        let open = reopened(&dir).tasks().get(id).unwrap().clone();
        assert_eq!(open.status, Status::Todo);
        assert_eq!(open.completed_at, None);
    }

    #[test]
    fn completing_never_changes_other_tasks() {
        let (_dir, mut session) = session();
        let parent = session.capture("Move house").unwrap();
        let child = session.capture("Pack books").unwrap();
        edit(&mut session, child, |task| task.parent_id = Some(parent)).unwrap();
        assert_eq!(session.completion_warnings(parent).open_subtasks, [child]);
        session.complete(parent).unwrap();
        assert_eq!(session.tasks().get(child).unwrap().status, Status::Inbox);
    }

    #[test]
    fn a_draft_made_before_an_outside_change_conflicts() {
        let (_dir, mut session) = session();
        let id = session.capture("Call the bank").unwrap();
        let mut draft = session.draft(id).unwrap();
        draft.task.body = "Mine.\n".into();

        let file = path(&session, id);
        fs::write(&file, fs::read_to_string(&file).unwrap() + "Theirs.\n").unwrap();
        session.refresh().unwrap();
        assert_eq!(
            session.tasks().get(id).unwrap().body,
            "Theirs.\n",
            "shows up in the app"
        );

        let error = session
            .save(draft.clone(), Overwrite::IfUnchanged)
            .unwrap_err();
        assert!(
            matches!(error, AppError::Conflict { deleted: false, .. }),
            "{error}"
        );
        session.save(draft, Overwrite::Anyway).unwrap();
        assert!(fs::read_to_string(&file).unwrap().ends_with("Mine.\n"));
    }

    #[test]
    fn an_outside_change_not_yet_seen_also_conflicts() {
        let (_dir, mut session) = session();
        let id = session.capture("Call the bank").unwrap();
        let file = path(&session, id);
        fs::write(&file, fs::read_to_string(&file).unwrap() + "Theirs.\n").unwrap();
        let error = edit(&mut session, id, |task| task.body = "Mine.\n".into()).unwrap_err();
        assert!(matches!(error, AppError::Conflict { .. }));
        assert!(fs::read_to_string(&file).unwrap().ends_with("Theirs.\n"));
    }

    #[test]
    fn deleting_moves_the_file_to_the_trash_and_subtasks_to_the_top() {
        let (dir, mut session) = session();
        let parent = session.capture("Move house").unwrap();
        let child = session.capture("Pack books").unwrap();
        let other = session.capture("Hand over keys").unwrap();
        edit(&mut session, child, |task| task.parent_id = Some(parent)).unwrap();
        edit(&mut session, other, |task| task.depends_on = vec![parent]).unwrap();

        let deleted = session
            .delete(
                parent,
                SubtaskChoice::MoveToTopLevel,
                Overwrite::IfUnchanged,
            )
            .unwrap();
        assert_eq!(deleted.trashed.len(), 1);
        assert_eq!(deleted.moved_to_top_level, [child]);
        assert_eq!(deleted.unlinked, [other]);

        let again = reopened(&dir);
        assert!(!again.tasks().contains(parent));
        assert_eq!(again.tasks().get(child).unwrap().parent_id, None);
        assert_eq!(again.tasks().get(other).unwrap().depends_on, []);
        assert_eq!(again.tasks().problems(), []);
        assert_eq!(
            fs::read_dir(again.workspace().trash_dir()).unwrap().count(),
            1
        );
    }

    #[test]
    fn deleting_can_take_subtasks_along() {
        let (dir, mut session) = session();
        let parent = session.capture("Move house").unwrap();
        let child = session.capture("Pack books").unwrap();
        edit(&mut session, child, |task| task.parent_id = Some(parent)).unwrap();
        let deleted = session
            .delete(parent, SubtaskChoice::Delete, Overwrite::IfUnchanged)
            .unwrap();
        assert_eq!(deleted.trashed.len(), 2);
        assert!(reopened(&dir).tasks().is_empty());
    }

    #[test]
    fn deleting_checks_every_file_before_changing_any() {
        let (_dir, mut session) = session();
        let parent = session.capture("Move house").unwrap();
        let child = session.capture("Pack books").unwrap();
        edit(&mut session, child, |task| task.parent_id = Some(parent)).unwrap();
        let child_file = path(&session, child);
        fs::write(
            &child_file,
            fs::read_to_string(&child_file).unwrap() + "Edited.\n",
        )
        .unwrap();

        let error = session
            .delete(
                parent,
                SubtaskChoice::MoveToTopLevel,
                Overwrite::IfUnchanged,
            )
            .unwrap_err();
        assert!(matches!(error, AppError::Conflict { .. }));
        assert!(path(&session, parent).exists(), "nothing was trashed");
    }

    #[test]
    fn own_writes_are_not_outside_changes() {
        let (_dir, mut session) = session();
        let id = session.capture("Call the bank").unwrap();
        let old_name = session.file_name(id).unwrap().to_owned();
        edit(&mut session, id, |task| {
            task.title = Title::new("Call the bank again").unwrap();
        })
        .unwrap();
        let new_name = session.file_name(id).unwrap().to_owned();
        let event = WatchEvent::Changed(BTreeSet::from([old_name, new_name]));
        assert!(!session.apply(&event).unwrap());
    }

    #[test]
    fn outside_changes_reload_the_workspace() {
        let (_dir, mut session) = session();
        let id = session.capture("Call the bank").unwrap();
        let name = session.file_name(id).unwrap().to_owned();
        let file = path(&session, id);
        fs::write(&file, fs::read_to_string(&file).unwrap() + "Theirs.\n").unwrap();
        assert!(
            session
                .apply(&WatchEvent::Changed(BTreeSet::from([name.clone()])))
                .unwrap()
        );
        assert_eq!(session.tasks().get(id).unwrap().body, "Theirs.\n");

        fs::remove_file(&file).unwrap();
        assert!(
            session
                .apply(&WatchEvent::Changed(BTreeSet::from([name])))
                .unwrap()
        );
        assert!(!session.tasks().contains(id));
    }

    #[test]
    fn new_and_broken_files_from_elsewhere_show_up() {
        let (_dir, mut session) = session();
        let tasks = session.workspace().tasks_dir();
        fs::write(tasks.join("broken.md"), "no front matter").unwrap();
        let event = WatchEvent::Changed(BTreeSet::from(["broken.md".to_owned()]));
        assert!(session.apply(&event).unwrap());
        assert_eq!(session.file_problems().len(), 1);

        fs::remove_file(tasks.join("broken.md")).unwrap();
        assert!(session.apply(&event).unwrap());
        assert_eq!(session.file_problems(), []);
    }
}
