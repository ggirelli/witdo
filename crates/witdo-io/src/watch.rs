//! Noticing changes made to task files outside witdo.

use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use notify_debouncer_full::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};

use crate::workspace::{Workspace, WorkspaceError};

/// How long changes to a file must settle before they are reported, so that an editor saving
/// in several steps gives one event.
const SETTLE: Duration = Duration::from_millis(300);

/// Something happened in `tasks/`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WatchEvent {
    /// Names in `tasks/` that were created, changed, renamed or removed. Includes files
    /// witdo wrote itself, which callers recognise by their hash.
    Changed(BTreeSet<String>),
    /// Watching failed; changes may have been missed, so a fresh scan is in order.
    Error(String),
}

/// Watches a workspace's `tasks/` until dropped.
pub struct Watcher {
    _debouncer: Debouncer<RecommendedWatcher, RecommendedCache>,
}

impl Workspace {
    /// Starts watching `tasks/`. `on_event` is called on a background thread.
    ///
    /// # Errors
    ///
    /// When the operating system refuses to watch the directory.
    pub fn watch(
        &self,
        mut on_event: impl FnMut(WatchEvent) + Send + 'static,
    ) -> Result<Watcher, WorkspaceError> {
        let tasks = self.tasks_dir();
        let tasks_for_events = tasks.clone();
        let mut debouncer =
            new_debouncer(
                SETTLE,
                None,
                move |result: DebounceEventResult| match result {
                    Ok(events) => {
                        let names: BTreeSet<String> = events
                            .iter()
                            .flat_map(|event| &event.paths)
                            .filter_map(|path| name_in(&tasks_for_events, path))
                            .collect();
                        if !names.is_empty() {
                            on_event(WatchEvent::Changed(names));
                        }
                    }
                    Err(errors) => {
                        let messages: Vec<String> =
                            errors.iter().map(ToString::to_string).collect();
                        on_event(WatchEvent::Error(messages.join("; ")));
                    }
                },
            )
            .map_err(|error| watch_error(&tasks, error))?;
        debouncer
            .watch(&tasks, RecursiveMode::NonRecursive)
            .map_err(|error| watch_error(&tasks, error))?;
        Ok(Watcher {
            _debouncer: debouncer,
        })
    }
}

/// The name of an entry directly in `tasks/`, or `None` for anything else.
fn name_in(tasks: &Path, path: &Path) -> Option<String> {
    (path.parent()? == tasks)
        .then(|| path.file_name()?.to_str().map(str::to_owned))
        .flatten()
}

fn watch_error(path: &Path, error: notify_debouncer_full::notify::Error) -> WorkspaceError {
    WorkspaceError::Io {
        path: path.to_owned(),
        action: "watched for changes",
        source: std::io::Error::other(error),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::mpsc;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn reports_files_changed_in_tasks() {
        let dir = TempDir::new().unwrap();
        let workspace = Workspace::init(dir.path(), None).unwrap();
        let (sender, events) = mpsc::channel();
        let _watcher = workspace
            .watch(move |event| {
                let _ = sender.send(event);
            })
            .unwrap();

        fs::write(workspace.tasks_dir().join("call-the-bank.md"), "---\n").unwrap();
        fs::write(workspace.tmp_dir().join("elsewhere.tmp"), "not watched").unwrap();

        let mut seen = BTreeSet::new();
        while !seen.contains("call-the-bank.md") {
            match events.recv_timeout(Duration::from_secs(10)) {
                Ok(WatchEvent::Changed(names)) => seen.extend(names),
                Ok(WatchEvent::Error(error)) => panic!("{error}"),
                Err(error) => panic!("no event for the new file ({error}); saw {seen:?}"),
            }
        }
        assert_eq!(seen.into_iter().collect::<Vec<_>>(), ["call-the-bank.md"]);
    }

    #[test]
    fn only_names_directly_in_tasks_count() {
        let tasks = Path::new("/w/tasks");
        assert_eq!(
            name_in(tasks, Path::new("/w/tasks/a.md")).as_deref(),
            Some("a.md")
        );
        assert_eq!(name_in(tasks, Path::new("/w/tasks/sub/a.md")), None);
        assert_eq!(name_in(tasks, Path::new("/w/tasks")), None);
        assert_eq!(name_in(tasks, Path::new("/w/.witdo/tmp/x.tmp")), None);
    }
}
