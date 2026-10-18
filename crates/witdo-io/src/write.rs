//! Changing task files safely: atomic writes through `.witdo/tmp/`, renames when a title
//! changes, the trash, and refusing to overwrite changes made outside witdo.

use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use witdo_core::{Task, TaskId};

use crate::file_name::{file_name, slug};
use crate::front_matter::{self, SerializeError};
use crate::workspace::{TaskFile, Workspace, content_hash};

/// Why a task file was not written. The file on disk is unchanged.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("{file} was changed outside witdo since it was loaded.")]
    ChangedOnDisk { file: String },
    #[error("{file} was deleted outside witdo since it was loaded.")]
    DeletedOnDisk { file: String },
    #[error(transparent)]
    Serialize(#[from] SerializeError),
    #[error("{} could not be {action}: {source}", path.display())]
    Io {
        path: PathBuf,
        action: &'static str,
        source: io::Error,
    },
}

impl WriteError {
    /// Whether the file changed outside witdo, so the person should choose between reloading it
    /// and overwriting it.
    #[must_use]
    pub fn is_conflict(&self) -> bool {
        matches!(
            self,
            Self::ChangedOnDisk { .. } | Self::DeletedOnDisk { .. }
        )
    }

    fn io(path: &Path, action: &'static str) -> impl FnOnce(io::Error) -> Self {
        let path = path.to_owned();
        move |source| Self::Io {
            path,
            action,
            source,
        }
    }
}

/// Whether to write over a file that changed outside witdo since it was loaded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Overwrite {
    /// Stop with a conflict instead.
    #[default]
    IfUnchanged,
    /// The person confirmed: write anyway.
    Anyway,
}

impl Workspace {
    /// Writes a new task to a new file named after its title.
    ///
    /// # Errors
    ///
    /// When the file cannot be written; nothing is left behind then.
    pub fn create(&self, task: &Task) -> Result<TaskFile, WriteError> {
        let tasks = self.tasks_dir();
        let name = file_name(&task.title, task.id, |name| tasks.join(name).exists());
        self.write(task, TaskFile::new(name), None)
    }

    /// Writes a changed task over its file. A file witdo named is renamed when the title
    /// changes; a name the person chose is kept.
    ///
    /// # Errors
    ///
    /// A conflict when the file changed or went away outside witdo (unless `overwrite` says to
    /// go ahead), or when it cannot be written. The file is unchanged then.
    pub fn save(
        &self,
        task: &Task,
        file: &TaskFile,
        overwrite: Overwrite,
    ) -> Result<TaskFile, WriteError> {
        if overwrite == Overwrite::IfUnchanged {
            self.check_unchanged(file)?;
        }
        let name = self.name_after_save(task, &file.name);
        let saved = TaskFile {
            name,
            ..file.clone()
        };
        self.write(task, saved, Some(&file.name))
    }

    /// Moves a task's file to `.witdo/trash/`, adding the time to its name. Returns the name
    /// in the trash, or `None` when the file was already gone and `overwrite` said to go ahead.
    ///
    /// # Errors
    ///
    /// A conflict when the file changed or went away outside witdo (unless `overwrite` says to
    /// go ahead), or when it cannot be moved.
    pub fn trash(
        &self,
        file: &TaskFile,
        overwrite: Overwrite,
        now: Timestamp,
    ) -> Result<Option<String>, WriteError> {
        match self.check_unchanged(file) {
            Err(WriteError::DeletedOnDisk { .. }) if overwrite == Overwrite::Anyway => {
                return Ok(None);
            }
            Err(conflict) if overwrite == Overwrite::IfUnchanged => return Err(conflict),
            _ => {}
        }
        let stem = file.name.strip_suffix(".md").unwrap_or(&file.name);
        let stamp = now.strftime("%Y%m%dT%H%M%SZ");
        let trash = self.trash_dir();
        let mut name = format!("{stem}--deleted-{stamp}.md");
        let mut copy = 1;
        while trash.join(&name).exists() {
            copy += 1;
            name = format!("{stem}--deleted-{stamp}-{copy}.md");
        }
        let from = self.tasks_dir().join(&file.name);
        fs::rename(&from, trash.join(&name))
            .map_err(WriteError::io(&from, "moved to the trash"))?;
        sync_dir(&self.tasks_dir());
        sync_dir(&trash);
        Ok(Some(name))
    }

    fn check_unchanged(&self, file: &TaskFile) -> Result<(), WriteError> {
        let path = self.tasks_dir().join(&file.name);
        match fs::read(&path) {
            Ok(bytes) if content_hash(&bytes) == file.hash => Ok(()),
            Ok(_) => Err(WriteError::ChangedOnDisk {
                file: file.name.clone(),
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Err(WriteError::DeletedOnDisk {
                    file: file.name.clone(),
                })
            }
            Err(error) => Err(WriteError::io(&path, "read")(error)),
        }
    }

    fn name_after_save(&self, task: &Task, current: &str) -> String {
        match witdo_slug(current, task.id) {
            Some(current_slug) if current_slug != slug(task.title.as_str()) => {
                let tasks = self.tasks_dir();
                file_name(&task.title, task.id, |name| {
                    name != current && tasks.join(name).exists()
                })
            }
            _ => current.to_owned(),
        }
    }

    /// Writes the task to `.witdo/tmp/`, then moves it into place as `file.name`, moving the
    /// file away from `previous` first when the name changes.
    fn write(
        &self,
        task: &Task,
        mut file: TaskFile,
        previous: Option<&str>,
    ) -> Result<TaskFile, WriteError> {
        let text = front_matter::serialize(task, &file.extras)?;
        let tmp = self.write_tmp(text.as_bytes())?;
        let tasks = self.tasks_dir();
        let target = tasks.join(&file.name);
        let renamed_from = match previous {
            Some(previous) if previous != file.name && tasks.join(previous).exists() => {
                // Rename first, so a crash in between leaves one file with the old contents
                // rather than two with the same ID.
                let previous = tasks.join(previous);
                if let Err(error) = fs::rename(&previous, &target) {
                    let _ = fs::remove_file(&tmp);
                    return Err(WriteError::io(&previous, "renamed")(error));
                }
                Some(previous)
            }
            _ => None,
        };
        if let Err(error) = fs::rename(&tmp, &target) {
            let _ = fs::remove_file(&tmp);
            if let Some(previous) = renamed_from {
                let _ = fs::rename(&target, previous);
            }
            return Err(WriteError::io(&target, "written")(error));
        }
        sync_dir(&tasks);
        file.hash = content_hash(text.as_bytes());
        file.extras.written(task);
        Ok(file)
    }

    fn write_tmp(&self, bytes: &[u8]) -> Result<PathBuf, WriteError> {
        let path = self.tmp_dir().join(format!("{}.tmp", TaskId::generate()));
        let written = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .and_then(|mut tmp| {
                tmp.write_all(bytes)?;
                tmp.sync_all()
            });
        written.map_err(|error| {
            let _ = fs::remove_file(&path);
            WriteError::io(&path, "written")(error)
        })?;
        Ok(path)
    }
}

impl TaskFile {
    fn new(name: String) -> Self {
        Self {
            name,
            hash: 0,
            extras: front_matter::Extras::default(),
        }
    }
}

/// The slug of a name witdo gave the task (`<slug>--<short id>.md`), or `None` for a name the
/// person chose.
fn witdo_slug(name: &str, id: TaskId) -> Option<&str> {
    let (slug, short_id) = name.strip_suffix(".md")?.rsplit_once("--")?;
    (short_id.len() >= 8 && short_id == id.suffix(short_id.len())).then_some(slug)
}

/// Makes a rename in `dir` durable. Best effort: the rename itself already happened, and
/// reporting a failure here would wrongly suggest it had not.
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(dir) = fs::File::open(dir) {
        let _ = dir.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;
    use witdo_core::Title;

    use super::*;

    fn workspace() -> (TempDir, Workspace) {
        let dir = TempDir::new().unwrap();
        let workspace = Workspace::init(dir.path(), None).unwrap();
        (dir, workspace)
    }

    fn task(title: &str) -> Task {
        Task::capture(
            "01JA4XKZ7Q3M8N2P5R4XK9Q2MB".parse().unwrap(),
            Title::new(title).unwrap(),
            "2026-10-18T10:00:00Z".parse().unwrap(),
        )
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    fn read(workspace: &Workspace, name: &str) -> String {
        fs::read_to_string(workspace.tasks_dir().join(name)).unwrap()
    }

    #[test]
    fn create_writes_a_readable_file_named_after_the_title() {
        let (_dir, workspace) = workspace();
        let file = workspace.create(&task("Call the bank")).unwrap();
        assert_eq!(file.name, "call-the-bank--4xk9q2mb.md");
        let scan = workspace.scan().unwrap();
        assert_eq!(scan.tasks.len(), 1);
        assert_eq!(scan.tasks[0].task, task("Call the bank"));
        assert_eq!(scan.tasks[0].file, file);
        assert_eq!(names(&workspace.tmp_dir()), Vec::<String>::new());
    }

    #[test]
    fn create_never_overwrites_another_file() {
        let (_dir, workspace) = workspace();
        fs::write(
            workspace.tasks_dir().join("call-the-bank--4xk9q2mb.md"),
            "mine",
        )
        .unwrap();
        let file = workspace.create(&task("Call the bank")).unwrap();
        assert_eq!(file.name, "call-the-bank--r4xk9q2mb.md");
        assert_eq!(read(&workspace, "call-the-bank--4xk9q2mb.md"), "mine");
    }

    #[test]
    fn save_keeps_the_name_while_the_title_stays() {
        let (_dir, workspace) = workspace();
        let mut task = task("Call the bank");
        let file = workspace.create(&task).unwrap();
        task.body = "Ask about the loan.\n".into();
        let saved = workspace
            .save(&task, &file, Overwrite::IfUnchanged)
            .unwrap();
        assert_eq!(saved.name, file.name);
        assert!(read(&workspace, &saved.name).ends_with("---\n\nAsk about the loan.\n"));
        assert_eq!(workspace.read(&saved.name).unwrap().file, saved);
    }

    #[test]
    fn save_renames_the_file_when_the_title_changes() {
        let (_dir, workspace) = workspace();
        let mut task = task("Call the bank");
        let file = workspace.create(&task).unwrap();
        task.title = Title::new("Call the bank about the loan").unwrap();
        let saved = workspace
            .save(&task, &file, Overwrite::IfUnchanged)
            .unwrap();
        assert_eq!(saved.name, "call-the-bank-about-the-loan--4xk9q2mb.md");
        assert_eq!(
            names(&workspace.tasks_dir()),
            std::slice::from_ref(&saved.name)
        );
        assert_eq!(workspace.read(&saved.name).unwrap().task, task);
    }

    #[test]
    fn save_keeps_names_the_person_chose() {
        let (_dir, workspace) = workspace();
        let mut task = task("Call the bank");
        let file = workspace.create(&task).unwrap();
        fs::rename(
            workspace.tasks_dir().join(&file.name),
            workspace.tasks_dir().join("Bank.md"),
        )
        .unwrap();
        let file = workspace.read("Bank.md").unwrap().file;
        task.title = Title::new("Call the bank again").unwrap();
        let saved = workspace
            .save(&task, &file, Overwrite::IfUnchanged)
            .unwrap();
        assert_eq!(saved.name, "Bank.md");
    }

    #[test]
    fn save_keeps_unknown_fields_and_notes() {
        let (_dir, workspace) = workspace();
        let file = workspace.create(&task("Call the bank")).unwrap();
        let path = workspace.tasks_dir().join(&file.name);
        let text = fs::read_to_string(&path)
            .unwrap()
            .replace("tags: []\n", "tags: []\nenergy: 3\n")
            + "Notes, *as written*.\n\n";
        fs::write(&path, &text).unwrap();
        let loaded = workspace.read(&file.name).unwrap();
        let mut task = loaded.task.clone();
        task.priority = witdo_core::Priority::High;
        workspace
            .save(&task, &loaded.file, Overwrite::IfUnchanged)
            .unwrap();
        let written = read(&workspace, &file.name);
        assert_eq!(written, text.replace("\"normal\"", "\"high\""));
    }

    #[test]
    fn save_stops_when_the_file_changed_outside_witdo() {
        let (_dir, workspace) = workspace();
        let mut task = task("Call the bank");
        let file = workspace.create(&task).unwrap();
        let path = workspace.tasks_dir().join(&file.name);
        let theirs = fs::read_to_string(&path).unwrap() + "Their notes.\n";
        fs::write(&path, &theirs).unwrap();

        task.body = "My notes.\n".into();
        let error = workspace
            .save(&task, &file, Overwrite::IfUnchanged)
            .unwrap_err();
        assert!(error.is_conflict());
        assert_eq!(fs::read_to_string(&path).unwrap(), theirs, "untouched");

        workspace.save(&task, &file, Overwrite::Anyway).unwrap();
        assert!(fs::read_to_string(&path).unwrap().ends_with("My notes.\n"));
    }

    #[test]
    fn save_stops_when_the_file_was_deleted_outside_witdo() {
        let (_dir, workspace) = workspace();
        let task = task("Call the bank");
        let file = workspace.create(&task).unwrap();
        fs::remove_file(workspace.tasks_dir().join(&file.name)).unwrap();
        let error = workspace
            .save(&task, &file, Overwrite::IfUnchanged)
            .unwrap_err();
        assert!(matches!(error, WriteError::DeletedOnDisk { .. }));
        workspace.save(&task, &file, Overwrite::Anyway).unwrap();
        assert_eq!(names(&workspace.tasks_dir()), [file.name]);
    }

    #[test]
    fn a_failed_write_leaves_the_file_as_it_was() {
        let (_dir, workspace) = workspace();
        let mut task = task("Call the bank");
        let file = workspace.create(&task).unwrap();
        let before = read(&workspace, &file.name);
        fs::remove_dir(workspace.tmp_dir()).unwrap();
        task.title = Title::new("Renamed").unwrap();
        let error = workspace
            .save(&task, &file, Overwrite::IfUnchanged)
            .unwrap_err();
        assert!(matches!(error, WriteError::Io { .. }), "{error}");
        assert_eq!(read(&workspace, &file.name), before);
        assert_eq!(names(&workspace.tasks_dir()), [file.name]);
    }

    #[test]
    fn trash_moves_the_file_with_the_time_in_its_name() {
        let (_dir, workspace) = workspace();
        let file = workspace.create(&task("Call the bank")).unwrap();
        let now = "2026-10-20T08:30:00Z".parse().unwrap();
        let trashed = workspace.trash(&file, Overwrite::IfUnchanged, now).unwrap();
        assert_eq!(
            trashed.as_deref(),
            Some("call-the-bank--4xk9q2mb--deleted-20261020T083000Z.md")
        );
        assert_eq!(names(&workspace.tasks_dir()), Vec::<String>::new());
        assert_eq!(names(&workspace.trash_dir()), [trashed.unwrap()]);

        let again = workspace.create(&task("Call the bank")).unwrap();
        let second = workspace
            .trash(&again, Overwrite::IfUnchanged, now)
            .unwrap();
        assert_eq!(
            second.as_deref(),
            Some("call-the-bank--4xk9q2mb--deleted-20261020T083000Z-2.md")
        );
    }

    #[test]
    fn trash_stops_when_the_file_changed_outside_witdo() {
        let (_dir, workspace) = workspace();
        let file = workspace.create(&task("Call the bank")).unwrap();
        let path = workspace.tasks_dir().join(&file.name);
        fs::write(&path, fs::read_to_string(&path).unwrap() + "More.\n").unwrap();
        let now = Timestamp::UNIX_EPOCH;
        assert!(
            workspace
                .trash(&file, Overwrite::IfUnchanged, now)
                .unwrap_err()
                .is_conflict()
        );
        assert!(path.exists());
        assert!(
            workspace
                .trash(&file, Overwrite::Anyway, now)
                .unwrap()
                .is_some()
        );

        assert_eq!(
            workspace.trash(&file, Overwrite::Anyway, now).unwrap(),
            None,
            "already gone"
        );
    }

    #[test]
    fn recognises_names_witdo_gave() {
        let id: TaskId = "01JA4XKZ7Q3M8N2P5R4XK9Q2MB".parse().unwrap();
        assert_eq!(
            witdo_slug("call-the-bank--4xk9q2mb.md", id),
            Some("call-the-bank")
        );
        assert_eq!(
            witdo_slug("call-the-bank--r4xk9q2mb.md", id),
            Some("call-the-bank")
        );
        assert_eq!(witdo_slug("call-the-bank--aaaaaaaa.md", id), None);
        assert_eq!(witdo_slug("Bank.md", id), None);
    }
}
