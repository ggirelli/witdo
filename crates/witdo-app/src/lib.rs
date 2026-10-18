//! Use cases: creating, editing, completing, reopening and deleting tasks, and handling
//! workspaces and profiles. The UI talks to this crate only.

mod problems;
mod session;

/// The task model, for the UIs.
pub mod model {
    pub use witdo_core::*;
}

use std::fmt;
use std::sync::Arc;

use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;
pub use problems::{ProblemKind, ProblemReport};
pub use session::{Deleted, Draft, Session};
use witdo_core::{EditError, EmptyText, TaskId};
pub use witdo_io::watch::{WatchEvent, Watcher};
use witdo_io::workspace::WorkspaceError;
pub use witdo_io::workspace::{DirectoryState, FileProblem, IgnoredReason};
pub use witdo_io::write::Overwrite;
use witdo_io::write::WriteError;

/// Something a use case could not do, said so that it can be shown to the person.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("The {} is empty.", .0.0)]
    Title(#[from] EmptyText),
    #[error(transparent)]
    Edit(#[from] EditError),
    #[error("There is no task with the ID {0}.")]
    UnknownTask(TaskId),
    /// The file changed (or was deleted) outside witdo; the person chooses to reload or
    /// overwrite.
    #[error("{file} was {} outside witdo since it was loaded.", if *deleted { "deleted" } else { "changed" })]
    Conflict { file: String, deleted: bool },
    #[error(transparent)]
    Write(WriteError),
    #[error(transparent)]
    Workspace(#[from] WorkspaceError),
}

impl AppError {
    /// Whether the person should be offered "Reload" or "Overwrite".
    #[must_use]
    pub fn is_conflict(&self) -> bool {
        matches!(self, Self::Conflict { .. })
    }
}

/// Where the use cases get the time from: the system, or a fixed time in tests.
#[derive(Clone)]
pub struct Clock(Arc<dyn Fn() -> Timestamp + Send + Sync>);

impl Clock {
    #[must_use]
    pub fn system() -> Self {
        Self(Arc::new(Timestamp::now))
    }

    #[must_use]
    pub fn fixed(now: Timestamp) -> Self {
        Self(Arc::new(move || now))
    }

    /// Now, in whole seconds, as task files store it.
    #[must_use]
    pub fn now(&self) -> Timestamp {
        let now = (self.0)();
        Timestamp::from_second(now.as_second()).unwrap_or(now)
    }

    /// Today's date in the system's time zone, for due dates.
    #[must_use]
    pub fn today(&self) -> Date {
        self.now().to_zoned(TimeZone::system()).date()
    }
}

impl fmt::Debug for Clock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Clock").field(&self.now()).finish()
    }
}
