//! A workspace: a self-contained directory of task files, marked by `.witdo/settings.toml`.

use std::collections::BTreeMap;
use std::fs;
use std::hash::{DefaultHasher, Hasher};
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use witdo_core::{Task, TaskId};

use crate::front_matter::{self, Extras, ParseError};

const TASKS: &str = "tasks";
const WITDO: &str = ".witdo";
const SETTINGS: &str = "settings.toml";
const TRASH: &str = "trash";
const TMP: &str = "tmp";
const SCHEMA_VERSION: u32 = 1;

/// An open workspace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workspace {
    root: PathBuf,
    settings: Settings,
}

/// What `.witdo/settings.toml` holds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    schema_version: u32,
    /// The name shown for the workspace; the directory's name when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// What a directory is, before opening or creating a workspace in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectoryState {
    Missing,
    Empty,
    /// Has files but is not a workspace: making it one needs the person's confirmation.
    NotEmpty,
    Workspace,
}

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("{} could not be {action}: {source}", path.display())]
    Io {
        path: PathBuf,
        action: &'static str,
        source: io::Error,
    },
    #[error("{} is not a witdo workspace (it has no .witdo/settings.toml).", .0.display())]
    NotAWorkspace(PathBuf),
    #[error("{} is already a witdo workspace.", .0.display())]
    AlreadyAWorkspace(PathBuf),
    #[error("{} could not be read: {message}", path.display())]
    BadSettings { path: PathBuf, message: String },
    #[error(
        "{} was made by a newer witdo (schema version {version}); update witdo to open it.",
        path.display()
    )]
    NewerSchema { path: PathBuf, version: u32 },
}

impl WorkspaceError {
    pub(crate) fn io(path: &Path, action: &'static str) -> impl FnOnce(io::Error) -> Self {
        let path = path.to_owned();
        move |source| Self::Io {
            path,
            action,
            source,
        }
    }
}

/// Tells what a directory is.
///
/// # Errors
///
/// When the directory exists but cannot be read.
pub fn inspect(path: &Path) -> Result<DirectoryState, WorkspaceError> {
    if path.join(WITDO).join(SETTINGS).is_file() {
        return Ok(DirectoryState::Workspace);
    }
    match fs::read_dir(path) {
        Ok(mut entries) => Ok(if entries.next().is_none() {
            DirectoryState::Empty
        } else {
            DirectoryState::NotEmpty
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(DirectoryState::Missing),
        Err(error) => Err(WorkspaceError::io(path, "read")(error)),
    }
}

impl Workspace {
    /// Makes a directory a workspace, creating it if needed. Files already there are left
    /// exactly as they are.
    ///
    /// # Errors
    ///
    /// When the directory is already a workspace, or the workspace's directories and settings
    /// cannot be created.
    pub fn init(root: &Path, name: Option<String>) -> Result<Self, WorkspaceError> {
        if inspect(root)? == DirectoryState::Workspace {
            return Err(WorkspaceError::AlreadyAWorkspace(root.to_owned()));
        }
        let witdo = root.join(WITDO);
        for dir in [root.join(TASKS), witdo.join(TRASH), witdo.join(TMP)] {
            fs::create_dir_all(&dir).map_err(WorkspaceError::io(&dir, "created"))?;
        }
        let settings = Settings {
            schema_version: SCHEMA_VERSION,
            name,
        };
        let path = witdo.join(SETTINGS);
        let text = toml::to_string(&settings).map_err(|error| WorkspaceError::BadSettings {
            path: path.clone(),
            message: error.to_string(),
        })?;
        fs::write(&path, text).map_err(WorkspaceError::io(&path, "written"))?;
        Ok(Self {
            root: canonical(root)?,
            settings,
        })
    }

    /// Opens an existing workspace, recreating its `tasks/`, trash and temporary directories if
    /// they went missing.
    ///
    /// # Errors
    ///
    /// When the directory is not a workspace, its settings cannot be read, or they come from a
    /// newer witdo.
    pub fn open(root: &Path) -> Result<Self, WorkspaceError> {
        let path = root.join(WITDO).join(SETTINGS);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(WorkspaceError::NotAWorkspace(root.to_owned()));
            }
            Err(error) => return Err(WorkspaceError::io(&path, "read")(error)),
        };
        let settings: Settings =
            toml::from_str(&text).map_err(|error| WorkspaceError::BadSettings {
                path: path.clone(),
                message: error.message().to_owned(),
            })?;
        if settings.schema_version > SCHEMA_VERSION {
            return Err(WorkspaceError::NewerSchema {
                path,
                version: settings.schema_version,
            });
        }
        let workspace = Self {
            root: canonical(root)?,
            settings,
        };
        for dir in [
            workspace.tasks_dir(),
            workspace.trash_dir(),
            workspace.tmp_dir(),
        ] {
            fs::create_dir_all(&dir).map_err(WorkspaceError::io(&dir, "created"))?;
        }
        Ok(workspace)
    }

    /// The workspace's directory, as an absolute path without symbolic links.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The name to show: the one in the settings, or else the directory's.
    #[must_use]
    pub fn name(&self) -> String {
        self.settings.name.clone().unwrap_or_else(|| {
            self.root.file_name().map_or_else(
                || self.root.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            )
        })
    }

    #[must_use]
    pub fn tasks_dir(&self) -> PathBuf {
        self.root.join(TASKS)
    }

    #[must_use]
    pub fn trash_dir(&self) -> PathBuf {
        self.root.join(WITDO).join(TRASH)
    }

    #[must_use]
    pub fn tmp_dir(&self) -> PathBuf {
        self.root.join(WITDO).join(TMP)
    }

    /// Reads every task file in `tasks/`. Files that are not valid tasks, and tasks that share
    /// an ID, are reported rather than loaded; nothing is changed on disk.
    ///
    /// # Errors
    ///
    /// When `tasks/` itself cannot be read. Problems with single files are in the result.
    pub fn scan(&self) -> Result<Scan, WorkspaceError> {
        let dir = self.tasks_dir();
        let entries = fs::read_dir(&dir).map_err(WorkspaceError::io(&dir, "read"))?;
        let mut names: Vec<(String, bool)> = Vec::new();
        let mut problems = Vec::new();
        for entry in entries {
            let entry = entry.map_err(WorkspaceError::io(&dir, "read"))?;
            let Ok(name) = entry.file_name().into_string() else {
                problems.push(FileProblem::Ignored {
                    file: entry.file_name().to_string_lossy().into_owned(),
                    reason: IgnoredReason::NameNotUtf8,
                });
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            let is_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
            names.push((name, is_dir));
        }
        names.sort();

        let mut by_id: BTreeMap<TaskId, Vec<LoadedTask>> = BTreeMap::new();
        for (name, is_dir) in names {
            if is_dir {
                problems.push(FileProblem::Ignored {
                    file: name,
                    reason: IgnoredReason::Directory,
                });
            } else if !is_task_file_name(&name) {
                problems.push(FileProblem::Ignored {
                    file: name,
                    reason: IgnoredReason::NotMarkdown,
                });
            } else {
                match self.read(&name) {
                    Ok(loaded) => by_id.entry(loaded.task.id).or_default().push(loaded),
                    Err(problem) => problems.push(problem),
                }
            }
        }

        let mut tasks = Vec::with_capacity(by_id.len());
        for (id, mut copies) in by_id {
            if copies.len() == 1 {
                tasks.extend(copies.pop());
            } else {
                problems.push(FileProblem::DuplicateId {
                    id,
                    files: copies.into_iter().map(|copy| copy.file.name).collect(),
                });
            }
        }
        tasks.sort_by(|a, b| a.file.name.cmp(&b.file.name));
        problems.sort_by(|a, b| a.file().cmp(b.file()));
        Ok(Scan { tasks, problems })
    }

    /// Reads one task file from `tasks/`.
    ///
    /// # Errors
    ///
    /// The problem with the file, when it cannot be read or is not a valid task.
    pub fn read(&self, name: &str) -> Result<LoadedTask, FileProblem> {
        let path = self.tasks_dir().join(name);
        let bytes = fs::read(&path).map_err(|error| FileProblem::Unreadable {
            file: name.to_owned(),
            message: error.to_string(),
        })?;
        let hash = content_hash(&bytes);
        let text = String::from_utf8(bytes).map_err(|_| FileProblem::Unreadable {
            file: name.to_owned(),
            message: "It is not UTF-8 text.".into(),
        })?;
        let (task, extras) =
            front_matter::parse(&text).map_err(|error| FileProblem::Malformed {
                file: name.to_owned(),
                error,
            })?;
        Ok(LoadedTask {
            task,
            file: TaskFile {
                name: name.to_owned(),
                hash,
                extras,
            },
        })
    }
}

impl Workspace {
    /// The hash of a file in `tasks/` as it is now, or `None` when there is no such file.
    ///
    /// # Errors
    ///
    /// When the file exists but cannot be read.
    pub fn file_hash(&self, name: &str) -> Result<Option<u64>, WorkspaceError> {
        let path = self.tasks_dir().join(name);
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(content_hash(&bytes))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(WorkspaceError::io(&path, "read")(error)),
        }
    }
}

fn canonical(path: &Path) -> Result<PathBuf, WorkspaceError> {
    fs::canonicalize(path).map_err(WorkspaceError::io(path, "found"))
}

/// Whether a name in `tasks/` is one witdo reads as a task: `*.md`, not hidden.
#[must_use]
pub fn is_task_file_name(name: &str) -> bool {
    !name.starts_with('.')
        && Path::new(name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}

/// Everything a scan found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scan {
    /// Valid tasks with unique IDs, by file name.
    pub tasks: Vec<LoadedTask>,
    /// By file name.
    pub problems: Vec<FileProblem>,
}

/// A task and the file it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedTask {
    pub task: Task,
    pub file: TaskFile,
}

/// What witdo remembers about a task's file: its name, a hash of the contents it last read or
/// wrote (to notice changes made elsewhere), and what the file holds besides the task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskFile {
    pub name: String,
    pub hash: u64,
    pub extras: Extras,
}

/// A file in `tasks/` that did not load as a task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileProblem {
    /// Not a valid task file; it is left alone.
    Malformed {
        file: String,
        error: ParseError,
    },
    /// Two or more files with the same ID, typically a copied file. None of them loads until
    /// the person resolves it.
    DuplicateId {
        id: TaskId,
        files: Vec<String>,
    },
    Unreadable {
        file: String,
        message: String,
    },
    Ignored {
        file: String,
        reason: IgnoredReason,
    },
}

impl FileProblem {
    /// The file the problem is about (the first, for duplicate IDs).
    #[must_use]
    pub fn file(&self) -> &str {
        match self {
            Self::Malformed { file, .. }
            | Self::Unreadable { file, .. }
            | Self::Ignored { file, .. } => file,
            Self::DuplicateId { files, .. } => files.first().map_or("", String::as_str),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IgnoredReason {
    /// `tasks/` is flat; witdo does not look inside subdirectories.
    Directory,
    NotMarkdown,
    NameNotUtf8,
}

/// A hash of a file's contents, to tell whether it changed since witdo last read or wrote it.
#[must_use]
pub fn content_hash(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    hasher.write(bytes);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    const TASK: &str = "---\nid: \"01JA4XKZ7Q3M8N2P5R4XK9Q2MB\"\ntitle: \"Call the bank\"\n\
        status: \"inbox\"\npriority: \"normal\"\ncreated_at: \"2026-10-18T10:00:00Z\"\n\
        updated_at: \"2026-10-18T10:00:00Z\"\n---\n";

    fn workspace() -> (TempDir, Workspace) {
        let dir = TempDir::new().unwrap();
        let workspace = Workspace::init(dir.path(), None).unwrap();
        (dir, workspace)
    }

    fn put(workspace: &Workspace, name: &str, text: &str) {
        fs::write(workspace.tasks_dir().join(name), text).unwrap();
    }

    fn other_id(text: &str, last: char) -> String {
        text.replace("Q2MB", &format!("Q2M{last}"))
    }

    #[test]
    fn tells_what_a_directory_is() {
        let dir = TempDir::new().unwrap();
        assert_eq!(
            inspect(&dir.path().join("nope")).unwrap(),
            DirectoryState::Missing
        );
        assert_eq!(inspect(dir.path()).unwrap(), DirectoryState::Empty);
        fs::write(dir.path().join("notes.txt"), "hi").unwrap();
        assert_eq!(inspect(dir.path()).unwrap(), DirectoryState::NotEmpty);
        Workspace::init(dir.path(), None).unwrap();
        assert_eq!(inspect(dir.path()).unwrap(), DirectoryState::Workspace);
    }

    #[test]
    fn init_lays_out_the_workspace_and_leaves_other_files_alone() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("notes.txt"), "hi").unwrap();
        let workspace = Workspace::init(dir.path(), Some("Work".into())).unwrap();
        assert!(workspace.tasks_dir().is_dir());
        assert!(workspace.trash_dir().is_dir());
        assert!(workspace.tmp_dir().is_dir());
        assert_eq!(
            fs::read_to_string(dir.path().join(".witdo/settings.toml")).unwrap(),
            "schema_version = 1\nname = \"Work\"\n"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("notes.txt")).unwrap(),
            "hi"
        );
        assert!(matches!(
            Workspace::init(dir.path(), None),
            Err(WorkspaceError::AlreadyAWorkspace(_))
        ));
    }

    #[test]
    fn creates_a_missing_directory() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("new/work");
        Workspace::init(&root, None).unwrap();
        assert_eq!(Workspace::open(&root).unwrap().name(), "work");
    }

    #[test]
    fn open_reads_the_settings() {
        let (dir, _) = workspace();
        let workspace = Workspace::open(dir.path()).unwrap();
        assert_eq!(workspace.root(), dir.path().canonicalize().unwrap());
        assert_eq!(
            workspace.name(),
            dir.path().file_name().unwrap().to_string_lossy()
        );
    }

    #[test]
    fn roots_are_absolute_and_canonical() {
        let (dir, _) = workspace();
        fs::create_dir(dir.path().join("sub")).unwrap();
        let workspace = Workspace::open(&dir.path().join("sub/..")).unwrap();
        assert_eq!(workspace.root(), dir.path().canonicalize().unwrap());
    }

    #[test]
    fn open_refuses_what_is_not_a_workspace() {
        let dir = TempDir::new().unwrap();
        let error = Workspace::open(dir.path()).unwrap_err();
        assert!(matches!(error, WorkspaceError::NotAWorkspace(_)));
        assert!(
            error
                .to_string()
                .ends_with("is not a witdo workspace (it has no .witdo/settings.toml).")
        );
    }

    #[test]
    fn open_refuses_bad_or_newer_settings() {
        let (dir, _) = workspace();
        let settings = dir.path().join(".witdo/settings.toml");
        fs::write(&settings, "schema_version = \"one\"\n").unwrap();
        assert!(matches!(
            Workspace::open(dir.path()),
            Err(WorkspaceError::BadSettings { .. })
        ));
        fs::write(&settings, "schema_version = 2\n").unwrap();
        assert!(matches!(
            Workspace::open(dir.path()),
            Err(WorkspaceError::NewerSchema { version: 2, .. })
        ));
    }

    #[test]
    fn open_recreates_missing_directories() {
        let (dir, workspace) = workspace();
        fs::remove_dir(workspace.tmp_dir()).unwrap();
        fs::remove_dir(workspace.tasks_dir()).unwrap();
        let workspace = Workspace::open(dir.path()).unwrap();
        assert!(workspace.tmp_dir().is_dir());
        assert_eq!(workspace.scan().unwrap(), Scan::default());
    }

    #[test]
    fn scan_loads_task_files_whatever_their_name() {
        let (_dir, workspace) = workspace();
        put(&workspace, "call-the-bank--4xk9q2mb.md", TASK);
        put(&workspace, "My Note.MD", &other_id(TASK, 'C'));
        let scan = workspace.scan().unwrap();
        assert_eq!(scan.problems, []);
        let names: Vec<&str> = scan.tasks.iter().map(|t| t.file.name.as_str()).collect();
        assert_eq!(names, ["My Note.MD", "call-the-bank--4xk9q2mb.md"]);
        assert_eq!(scan.tasks[1].file.hash, content_hash(TASK.as_bytes()));
    }

    #[test]
    fn scan_reports_files_it_cannot_load_and_changes_nothing() {
        let (_dir, workspace) = workspace();
        put(&workspace, "a-broken.md", "---\ntitle: x\n---\n");
        put(&workspace, "b-notes.txt", "hi");
        fs::create_dir(workspace.tasks_dir().join("c-archive")).unwrap();
        fs::write(
            workspace.tasks_dir().join("d-binary.md"),
            [0xff, 0xfe, 0x00],
        )
        .unwrap();
        put(&workspace, ".hidden.md", "ignored without a word");
        let scan = workspace.scan().unwrap();
        assert_eq!(scan.tasks, []);
        let problems: Vec<&FileProblem> = scan.problems.iter().collect();
        assert!(
            matches!(problems[0], FileProblem::Malformed { file, error } if file == "a-broken.md" && error.message.contains("`id`"))
        );
        assert_eq!(
            problems[1],
            &FileProblem::Ignored {
                file: "b-notes.txt".into(),
                reason: IgnoredReason::NotMarkdown
            }
        );
        assert_eq!(
            problems[2],
            &FileProblem::Ignored {
                file: "c-archive".into(),
                reason: IgnoredReason::Directory
            }
        );
        assert!(
            matches!(problems[3], FileProblem::Unreadable { file, .. } if file == "d-binary.md")
        );
        assert_eq!(problems.len(), 4);
        assert_eq!(
            fs::read_to_string(workspace.tasks_dir().join("a-broken.md")).unwrap(),
            "---\ntitle: x\n---\n"
        );
    }

    #[test]
    fn file_hash_tells_present_files_from_missing_ones() {
        let (_dir, workspace) = workspace();
        put(&workspace, "a.md", TASK);
        assert_eq!(
            workspace.file_hash("a.md").unwrap(),
            Some(content_hash(TASK.as_bytes()))
        );
        assert_eq!(workspace.file_hash("b.md").unwrap(), None);
    }

    #[test]
    fn scan_holds_back_every_copy_of_a_duplicate_id() {
        let (_dir, workspace) = workspace();
        put(&workspace, "call-the-bank--4xk9q2mb.md", TASK);
        put(&workspace, "call-the-bank--4xk9q2mb copy.md", TASK);
        put(&workspace, "other.md", &other_id(TASK, 'C'));
        let scan = workspace.scan().unwrap();
        assert_eq!(scan.tasks.len(), 1);
        assert_eq!(
            scan.problems,
            [FileProblem::DuplicateId {
                id: "01JA4XKZ7Q3M8N2P5R4XK9Q2MB".parse().unwrap(),
                files: vec![
                    "call-the-bank--4xk9q2mb copy.md".into(),
                    "call-the-bank--4xk9q2mb.md".into()
                ],
            }]
        );
    }
}
