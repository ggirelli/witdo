//! The desktop app's state and what each action does. The components in `view` draw it.

use std::path::{Path, PathBuf};

use witdo_app::model::{CompletionWarnings, Filter, SubtaskChoice, Task, TaskId, View};
use witdo_app::{
    AppError, Clock, DirectoryState, Draft, Layout, Opening, Overwrite, ProfileSession, Row,
    Session, WatchEvent,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub text: String,
    pub error: bool,
}

impl Message {
    fn info(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            error: false,
        }
    }

    fn error(error: &AppError) -> Self {
        Self {
            text: error.to_string(),
            error: true,
        }
    }
}

pub struct Model {
    pub profile: ProfileSession,
    pub clock: Clock,
    pub screen: Screen,
    pub message: Option<Message>,
    pub dialog: Option<Dialog>,
}

pub enum Screen {
    /// Picking or creating a workspace; `confirm` holds a folder waiting for a yes.
    Choose {
        confirm: Option<(PathBuf, DirectoryState)>,
    },
    Desk(Box<Desk>),
}

/// An open workspace and how it is being looked at.
pub struct Desk {
    pub session: Session,
    pub nav: Nav,
    pub layout: Layout,
    pub search: String,
    /// Everything but the text: status, priority, due date, category, tags, parent.
    pub filters: Filter,
    pub show_filters: bool,
    pub selected: Option<TaskId>,
    /// The selected task as it was when the person started editing it: saving an edit made
    /// from it stops with a conflict if the file changed since.
    base: Option<Draft>,
    /// The notes are being typed, so a reload must not replace the draft under them.
    notes_active: bool,
}

/// A sidebar entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Nav {
    View(View),
    Problems,
}

pub enum Dialog {
    Complete {
        id: TaskId,
        warnings: CompletionWarnings,
    },
    Delete {
        id: TaskId,
        choice: SubtaskChoice,
    },
    Conflict {
        file: String,
        deleted: bool,
        retry: Retry,
    },
}

pub enum Retry {
    Save(Box<Draft>),
    Delete(TaskId, SubtaskChoice),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    Reload,
    Overwrite,
}

impl Model {
    pub fn new(profile: ProfileSession, opening: Opening, clock: Clock) -> Self {
        let mut model = Self {
            profile,
            clock,
            screen: Screen::Choose { confirm: None },
            message: None,
            dialog: None,
        };
        match opening {
            Opening::Opened(session) => model.screen = Screen::Desk(Box::new(Desk::new(*session))),
            Opening::Choose => {}
            Opening::Confirm { path, state } => {
                model.screen = Screen::Choose {
                    confirm: Some((path, state)),
                };
            }
            Opening::Failed(error) => model.message = Some(Message::error(&error)),
        }
        model
    }

    pub fn desk(&self) -> Option<&Desk> {
        match &self.screen {
            Screen::Desk(desk) => Some(desk),
            Screen::Choose { .. } => None,
        }
    }

    fn desk_mut(&mut self) -> Option<&mut Desk> {
        match &mut self.screen {
            Screen::Desk(desk) => Some(desk),
            Screen::Choose { .. } => None,
        }
    }

    // Workspaces.

    pub fn switch_workspace(&mut self) {
        self.screen = Screen::Choose { confirm: None };
        self.dialog = None;
        self.message = None;
    }

    pub fn open_or_create(&mut self, path: &Path) {
        match ProfileSession::inspect(path) {
            Ok(DirectoryState::Workspace) => self.open(path),
            Ok(DirectoryState::Missing | DirectoryState::Empty) => self.create(path),
            Ok(state @ DirectoryState::NotEmpty) => {
                self.screen = Screen::Choose {
                    confirm: Some((path.to_owned(), state)),
                };
            }
            Err(error) => self.message = Some(Message::error(&error)),
        }
    }

    pub fn open(&mut self, path: &Path) {
        match self.profile.open_workspace(path, self.clock.clone()) {
            Ok(session) => {
                self.screen = Screen::Desk(Box::new(Desk::new(session)));
                self.message = None;
            }
            Err(error) => self.message = Some(Message::error(&error)),
        }
    }

    pub fn create(&mut self, path: &Path) {
        match self
            .profile
            .create_workspace(path, None, self.clock.clone())
        {
            Ok(session) => {
                self.message = Some(Message::info(format!(
                    "Created a workspace in {}.",
                    session.workspace().root().display()
                )));
                self.screen = Screen::Desk(Box::new(Desk::new(session)));
            }
            Err(error) => {
                self.screen = Screen::Choose { confirm: None };
                self.message = Some(Message::error(&error));
            }
        }
    }

    pub fn cancel_confirm(&mut self) {
        self.screen = Screen::Choose { confirm: None };
    }

    pub fn forget(&mut self, path: &Path) {
        self.message = Some(match self.profile.forget_workspace(path) {
            Ok(()) => Message::info(format!(
                "Forgot {}; its files are untouched.",
                path.display()
            )),
            Err(error) => Message::error(&error),
        });
    }

    /// Saves the window's size in the profile. It runs as the window closes, so a failure can
    /// only be reported on the terminal.
    pub fn remember_window_size(&mut self, width: u32, height: u32) {
        if let Err(error) = self.profile.set_window_size(width, height) {
            eprintln!("witdo: the window size could not be saved: {error}");
        }
    }

    // Looking around.

    pub fn set_nav(&mut self, nav: Nav) {
        if let Some(desk) = self.desk_mut() {
            desk.nav = nav;
        }
    }

    pub fn set_layout(&mut self, layout: Layout) {
        if let Some(desk) = self.desk_mut() {
            desk.layout = layout;
        }
    }

    pub fn set_search(&mut self, text: String) {
        if let Some(desk) = self.desk_mut() {
            desk.search = text;
        }
    }

    pub fn set_filters(&mut self, change: impl FnOnce(&mut Filter)) {
        if let Some(desk) = self.desk_mut() {
            change(&mut desk.filters);
        }
    }

    pub fn toggle_filters(&mut self) {
        if let Some(desk) = self.desk_mut() {
            desk.show_filters = !desk.show_filters;
        }
    }

    pub fn select(&mut self, id: Option<TaskId>) {
        if let Some(desk) = self.desk_mut() {
            desk.selected = id;
            desk.base = id.and_then(|id| desk.session.draft(id));
            desk.notes_active = false;
        }
    }

    /// Moves the selection up or down the list.
    pub fn step(&mut self, forward: bool) {
        let Some(desk) = self.desk() else { return };
        let rows = desk.rows();
        let index = desk
            .selected
            .and_then(|id| rows.iter().position(|row| row.id == id));
        let next = match index {
            None => 0,
            Some(index) if forward => (index + 1).min(rows.len().saturating_sub(1)),
            Some(index) => index.saturating_sub(1),
        };
        let id = rows.get(next).map(|row| row.id);
        self.select(id);
    }

    // Changing tasks.

    /// Adds a task to the inbox; returns whether it worked.
    pub fn capture(&mut self, title: &str) -> bool {
        let Some(desk) = self.desk_mut() else {
            return false;
        };
        match desk.session.capture(title) {
            Ok(_) => {
                self.message = Some(Message::info(format!(
                    "Added “{}” to the inbox.",
                    title.trim()
                )));
                true
            }
            Err(error) => {
                self.message = Some(Message::error(&error));
                false
            }
        }
    }

    /// Changes the selected task from the version the person is looking at, and saves it.
    pub fn edit(&mut self, change: impl FnOnce(&mut Task, &Clock) -> Result<(), AppError>) {
        let clock = self.clock.clone();
        let Some(desk) = self.desk_mut() else { return };
        let Some(mut draft) = desk.base.clone() else {
            return;
        };
        if let Err(error) = change(&mut draft.task, &clock) {
            self.message = Some(Message::error(&error));
            return;
        }
        self.save(draft, Overwrite::IfUnchanged);
    }

    fn save(&mut self, draft: Draft, overwrite: Overwrite) {
        let Some(desk) = self.desk_mut() else { return };
        let id = draft.id();
        match desk.session.save(draft.clone(), overwrite) {
            Ok(()) => {
                desk.base = desk.session.draft(id);
                self.message = None;
            }
            Err(AppError::Conflict { file, deleted }) => {
                self.dialog = Some(Dialog::Conflict {
                    file,
                    deleted,
                    retry: Retry::Save(Box::new(draft)),
                });
            }
            Err(error) => self.message = Some(Message::error(&error)),
        }
    }

    /// The person started typing notes.
    pub fn begin_notes(&mut self) {
        if let Some(desk) = self.desk_mut() {
            desk.notes_active = true;
        }
    }

    pub fn end_notes(&mut self) {
        if let Some(desk) = self.desk_mut() {
            desk.notes_active = false;
        }
    }

    /// Marks a task done (asking first if work under or before it is open), or reopens it.
    pub fn toggle_done(&mut self, id: TaskId) {
        self.select(Some(id));
        let Some(desk) = self.desk() else { return };
        let Some(task) = desk.session.tasks().get(id) else {
            return;
        };
        if task.is_open() {
            let warnings = desk.session.completion_warnings(id);
            if warnings.is_empty() {
                self.edit(|task, clock| {
                    task.complete(clock.now());
                    Ok(())
                });
            } else {
                self.dialog = Some(Dialog::Complete { id, warnings });
            }
        } else {
            self.edit(|task, clock| {
                task.reopen(clock.now());
                Ok(())
            });
        }
    }

    pub fn confirm_complete(&mut self) {
        if let Some(Dialog::Complete { id, .. }) = self.dialog.take() {
            self.select(Some(id));
            self.edit(|task, clock| {
                task.complete(clock.now());
                Ok(())
            });
        }
    }

    pub fn ask_delete(&mut self, id: TaskId) {
        self.dialog = Some(Dialog::Delete {
            id,
            choice: SubtaskChoice::default(),
        });
    }

    pub fn set_delete_choice(&mut self, choice: SubtaskChoice) {
        if let Some(Dialog::Delete {
            choice: current, ..
        }) = &mut self.dialog
        {
            *current = choice;
        }
    }

    pub fn confirm_delete(&mut self) {
        if let Some(Dialog::Delete { id, choice }) = self.dialog.take() {
            self.delete(id, choice, Overwrite::IfUnchanged);
        }
    }

    fn delete(&mut self, id: TaskId, choice: SubtaskChoice, overwrite: Overwrite) {
        let Some(desk) = self.desk_mut() else { return };
        let title = desk
            .session
            .tasks()
            .get(id)
            .map(|t| t.title.to_string())
            .unwrap_or_default();
        match desk.session.delete(id, choice, overwrite) {
            Ok(deleted) => {
                if desk
                    .selected
                    .is_some_and(|selected| !desk.session.tasks().contains(selected))
                {
                    desk.selected = None;
                    desk.base = None;
                }
                let count = deleted.trashed.len();
                self.message = Some(Message::info(if count <= 1 {
                    format!("Moved “{title}” to the trash.")
                } else {
                    format!("Moved “{title}” and {} subtasks to the trash.", count - 1)
                }));
            }
            Err(AppError::Conflict { file, deleted }) => {
                self.dialog = Some(Dialog::Conflict {
                    file,
                    deleted,
                    retry: Retry::Delete(id, choice),
                });
            }
            Err(error) => self.message = Some(Message::error(&error)),
        }
    }

    pub fn resolve_conflict(&mut self, resolution: Resolution) {
        let Some(Dialog::Conflict { retry, .. }) = self.dialog.take() else {
            return;
        };
        match (resolution, retry) {
            (Resolution::Reload, _) => {
                self.reload();
                self.message = Some(Message::info("Reloaded; your change was discarded."));
            }
            (Resolution::Overwrite, Retry::Save(draft)) => self.save(*draft, Overwrite::Anyway),
            (Resolution::Overwrite, Retry::Delete(id, choice)) => {
                self.delete(id, choice, Overwrite::Anyway);
            }
        }
    }

    pub fn close_dialog(&mut self) {
        self.dialog = None;
    }

    pub fn reload(&mut self) {
        let Some(desk) = self.desk_mut() else { return };
        let result = desk.session.refresh();
        desk.rebase(true);
        if let Err(error) = result {
            self.message = Some(Message::error(&error));
        }
    }

    /// Takes in a change the watcher saw.
    pub fn apply(&mut self, event: &WatchEvent) {
        let Some(desk) = self.desk_mut() else { return };
        match desk.session.apply(event) {
            Ok(true) => {
                desk.rebase(false);
                self.message = Some(Message::info("Loaded changes made outside witdo."));
            }
            Ok(false) => {}
            Err(error) => self.message = Some(Message::error(&error)),
        }
    }
}

impl Desk {
    fn new(session: Session) -> Self {
        Self {
            session,
            nav: Nav::View(View::Next),
            layout: Layout::List,
            search: String::new(),
            filters: Filter::default(),
            show_filters: false,
            selected: None,
            base: None,
            notes_active: false,
        }
    }

    pub fn filter(&self) -> Filter {
        Filter {
            text: self.search.clone(),
            ..self.filters.clone()
        }
    }

    /// How many filters (besides the search) are set.
    pub fn active_filters(&self) -> usize {
        let filters = &self.filters;
        [
            !filters.statuses.is_empty(),
            !filters.priorities.is_empty(),
            filters.due.is_some(),
            filters.category.is_some(),
            !filters.tags.is_empty(),
            filters.parent.is_some(),
        ]
        .into_iter()
        .filter(|set| *set)
        .count()
    }

    pub fn rows(&self) -> Vec<Row> {
        match &self.nav {
            Nav::View(view) => self.session.rows(view, &self.filter(), self.layout),
            Nav::Problems => Vec::new(),
        }
    }

    /// The version of the selected task the fields show, as of the last save or reload.
    pub fn revision(&self) -> u64 {
        self.base.as_ref().map_or(0, Draft::revision)
    }

    /// Takes the selected task's current version as the base for edits, unless notes are
    /// being typed (and `force` is not set): those then conflict on saving, as they should.
    fn rebase(&mut self, force: bool) {
        if let Some(id) = self.selected
            && (force || !self.notes_active)
        {
            self.base = self.session.draft(id);
            if self.base.is_none() {
                self.selected = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;
    use witdo_app::model::{Status, Title};
    use witdo_app::{Launch, Profiles, start};

    use super::*;

    fn model() -> (TempDir, Model) {
        let dir = TempDir::new().unwrap();
        let launch = Launch {
            workspace: Some(dir.path().join("w")),
            ..Launch::default()
        };
        let clock = Clock::fixed("2026-10-21T12:00:00Z".parse().unwrap());
        let (profile, opening) =
            start(&launch, Profiles::at(dir.path().join("config")), &clock).unwrap();
        let mut model = Model::new(profile, opening, clock);
        let Screen::Choose {
            confirm: Some((path, _)),
        } = &model.screen
        else {
            panic!("should ask")
        };
        let path = path.clone();
        model.create(&path);
        (dir, model)
    }

    fn only_task(model: &Model) -> TaskId {
        model
            .desk()
            .unwrap()
            .session
            .tasks()
            .iter()
            .next()
            .unwrap()
            .id
    }

    #[test]
    fn captures_and_edits_the_selected_task() {
        let (_dir, mut model) = model();
        assert!(model.capture("Call the bank"));
        let id = only_task(&model);
        model.select(Some(id));
        model.edit(|task, _| {
            task.title = Title::new("Call the bank again")?;
            Ok(())
        });
        let desk = model.desk().unwrap();
        assert_eq!(
            desk.session.tasks().get(id).unwrap().title.as_str(),
            "Call the bank again"
        );
        assert_ne!(desk.revision(), 0);
    }

    #[test]
    fn edits_build_on_each_other() {
        let (_dir, mut model) = model();
        model.capture("Call the bank");
        model.select(Some(only_task(&model)));
        model.edit(|task, _| {
            task.body = "First.\n".into();
            Ok(())
        });
        model.edit(|task, _| {
            task.body.push_str("Second.\n");
            Ok(())
        });
        assert!(model.dialog.is_none(), "no conflict with our own save");
        let id = only_task(&model);
        assert_eq!(
            model.desk().unwrap().session.tasks().get(id).unwrap().body,
            "First.\nSecond.\n"
        );
    }

    #[test]
    fn notes_typed_across_an_outside_change_conflict() {
        let (_dir, mut model) = model();
        model.capture("Call the bank");
        let id = only_task(&model);
        model.select(Some(id));
        model.begin_notes();

        let desk = model.desk().unwrap();
        let path = desk
            .session
            .workspace()
            .tasks_dir()
            .join(desk.session.file_name(id).unwrap());
        fs::write(&path, fs::read_to_string(&path).unwrap() + "Theirs.\n").unwrap();
        let event = WatchEvent::Changed([desk.session.file_name(id).unwrap().to_owned()].into());
        model.apply(&event);

        model.edit(|task, _| {
            "Mine.\n".clone_into(&mut task.body);
            Ok(())
        });
        assert!(matches!(model.dialog, Some(Dialog::Conflict { .. })));
        model.resolve_conflict(Resolution::Overwrite);
        assert!(fs::read_to_string(&path).unwrap().ends_with("Mine.\n"));
    }

    #[test]
    fn completing_with_open_subtasks_asks_first() {
        let (_dir, mut model) = model();
        model.capture("Move house");
        model.capture("Pack books");
        let desk = model.desk().unwrap();
        let find = |title: &str| {
            desk.session
                .tasks()
                .iter()
                .find(|t| t.title.as_str() == title)
                .unwrap()
                .id
        };
        let (parent, child) = (find("Move house"), find("Pack books"));
        model.select(Some(child));
        model.edit(|task, _| {
            task.parent_id = Some(parent);
            Ok(())
        });
        model.toggle_done(parent);
        assert!(matches!(model.dialog, Some(Dialog::Complete { .. })));
        model.confirm_complete();
        let tasks = model.desk().unwrap().session.tasks();
        assert_eq!(tasks.get(parent).unwrap().status, Status::Done);
        assert_eq!(tasks.get(child).unwrap().status, Status::Inbox);
    }

    #[test]
    fn deletes_after_confirming() {
        let (_dir, mut model) = model();
        model.capture("Call the bank");
        let id = only_task(&model);
        model.select(Some(id));
        model.ask_delete(id);
        model.confirm_delete();
        let desk = model.desk().unwrap();
        assert!(desk.session.tasks().is_empty());
        assert_eq!(desk.selected, None);
        assert_eq!(
            model.message.as_ref().unwrap().text,
            "Moved “Call the bank” to the trash."
        );
    }

    #[test]
    fn filters_narrow_the_list() {
        let (_dir, mut model) = model();
        model.capture("Call the bank");
        model.capture("Pack books");
        let id = model
            .desk()
            .unwrap()
            .session
            .tasks()
            .iter()
            .find(|t| t.title.as_str() == "Pack books")
            .unwrap()
            .id;
        model.select(Some(id));
        model.edit(|task, _| {
            task.priority = witdo_app::model::Priority::High;
            Ok(())
        });
        model.set_nav(Nav::View(View::All));
        model.set_filters(|filter| filter.priorities = vec![witdo_app::model::Priority::High]);
        let desk = model.desk().unwrap();
        assert_eq!(desk.active_filters(), 1);
        let titles: Vec<String> = desk.rows().into_iter().map(|row| row.title).collect();
        assert_eq!(titles, ["Pack books"]);
    }

    #[test]
    fn steps_through_the_list() {
        let (_dir, mut model) = model();
        model.capture("A");
        model.capture("B");
        model.set_nav(Nav::View(View::Inbox));
        model.step(true);
        let first = model.desk().unwrap().selected;
        model.step(true);
        let second = model.desk().unwrap().selected;
        assert_ne!(first, second);
        model.step(false);
        assert_eq!(model.desk().unwrap().selected, first);
    }
}
