//! The terminal app's state and what each key does. Drawing is in `ui`.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use witdo_app::model::{
    Category, CompletionWarnings, Filter, Priority, Status, SubtaskChoice, TaskId, Title, View,
};
use witdo_app::{
    AppError, Clock, DirectoryState, Draft, Layout, Linked, Opening, Overwrite, ProblemReport,
    ProfileSession, Row, Session, WatchEvent, Watcher, format_tags, parse_due, parse_tags,
};

use crate::input::TextInput;

/// Something the run loop has to do, outside the app's state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Quit,
    /// Suspend the terminal and edit this file in the person's editor.
    OpenEditor(PathBuf),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub text: String,
    pub error: bool,
}

impl Message {
    pub fn info(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            error: false,
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            error: true,
        }
    }
}

pub struct App {
    pub profile: ProfileSession,
    pub clock: Clock,
    pub screen: Screen,
    pub message: Option<Message>,
}

pub enum Screen {
    Choose(Chooser),
    Workspace(Box<Desk>),
}

/// Picking or creating a workspace.
#[derive(Default)]
pub struct Chooser {
    /// The profile's recent workspaces; one more row after them opens a folder by path.
    pub recent: Vec<PathBuf>,
    pub selected: usize,
    pub path: Option<TextInput>,
    /// A directory that is not a workspace yet, waiting for a yes.
    pub confirm: Option<(PathBuf, DirectoryState)>,
}

/// An open workspace.
pub struct Desk {
    pub session: Session,
    _watcher: Option<Watcher>,
    events: Receiver<WatchEvent>,
    pub nav: Vec<Nav>,
    pub nav_selected: usize,
    pub focus: Focus,
    pub layout: Layout,
    pub search: String,
    pub rows: Vec<Row>,
    pub selected: usize,
    pub problems: Vec<ProblemReport>,
    pub problem_selected: usize,
    /// The task the detail panel edits while it has the focus, wherever the list moves.
    pub detail_id: Option<TaskId>,
    pub field: Field,
    pub mode: Mode,
}

/// A sidebar entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Nav {
    View(View),
    Problems,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Sidebar,
    List,
    Detail,
}

pub enum Mode {
    Normal,
    Capture(TextInput),
    Search(TextInput),
    EditField(Field, TextInput),
    Pick(Picker),
    Confirm(Confirm),
    Help,
}

/// The detail panel's fields, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Title,
    Status,
    Priority,
    Due,
    Category,
    Tags,
    Parent,
    Prerequisites,
    Notes,
}

impl Field {
    pub const ALL: [Self; 9] = [
        Self::Title,
        Self::Status,
        Self::Priority,
        Self::Due,
        Self::Category,
        Self::Tags,
        Self::Parent,
        Self::Prerequisites,
        Self::Notes,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Title => "Title",
            Self::Status => "Status",
            Self::Priority => "Priority",
            Self::Due => "Due",
            Self::Category => "Category",
            Self::Tags => "Tags",
            Self::Parent => "Parent",
            Self::Prerequisites => "Blocked by",
            Self::Notes => "Notes",
        }
    }

    fn step(self, forward: bool) -> Self {
        let index = Self::ALL.iter().position(|&f| f == self).unwrap_or(0);
        let next = if forward {
            (index + 1).min(Self::ALL.len() - 1)
        } else {
            index.saturating_sub(1)
        };
        Self::ALL[next]
    }
}

/// Choosing a parent or prerequisites by title.
pub struct Picker {
    pub purpose: Purpose,
    pub input: TextInput,
    pub results: Vec<Linked>,
    /// For a parent, row 0 is "no parent" and results start at 1.
    pub selected: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Parent,
    Prerequisite,
}

pub enum Confirm {
    Complete {
        id: TaskId,
        warnings: CompletionWarnings,
    },
    Delete {
        id: TaskId,
        choice: SubtaskChoice,
    },
    /// A file changed outside witdo: reload it, or overwrite it with this change.
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

fn is_ctrl(key: KeyEvent, c: char) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char(c)
}

impl App {
    pub fn new(profile: ProfileSession, opening: Opening, clock: Clock) -> Self {
        let mut app = Self {
            profile,
            clock,
            screen: Screen::Choose(Chooser::default()),
            message: None,
        };
        match opening {
            Opening::Opened(session) => {
                app.screen = Screen::Workspace(Box::new(Desk::new(*session)));
            }
            Opening::Choose => app.choose(),
            Opening::Confirm { path, state } => {
                app.choose();
                if let Screen::Choose(chooser) = &mut app.screen {
                    chooser.confirm = Some((path, state));
                }
            }
            Opening::Failed(error) => {
                app.choose();
                app.message = Some(Message::error(error.to_string()));
            }
        }
        app
    }

    fn choose(&mut self) {
        self.screen = Screen::Choose(Chooser {
            recent: self.profile.recent_workspaces().to_vec(),
            ..Chooser::default()
        });
    }

    /// Takes in changes made outside witdo; call it between keys.
    pub fn tick(&mut self) {
        if let Screen::Workspace(desk) = &mut self.screen
            && let Some(message) = desk.poll()
        {
            self.message = Some(message);
        }
    }

    /// The person's editor closed after editing a task's file.
    pub fn after_editor(&mut self, result: std::io::Result<std::process::ExitStatus>) {
        if let Err(error) = result {
            self.message = Some(Message::error(format!(
                "The editor could not be started: {error}. Set $EDITOR to the one you use."
            )));
        }
        if let Screen::Workspace(desk) = &mut self.screen {
            if let Err(error) = desk.session.refresh() {
                self.message = Some(Message::error(error.to_string()));
            }
            desk.rebuild();
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        if key.kind != KeyEventKind::Press {
            return None;
        }
        if is_ctrl(key, 'c') {
            return Some(Action::Quit);
        }
        self.message = None;
        let clock = &self.clock;
        let outcome = match &mut self.screen {
            Screen::Choose(_) => return self.choose_key(key),
            Screen::Workspace(desk) => desk.handle_key(key, clock),
        };
        match outcome {
            DeskOutcome::Stay(message) => self.message = message,
            DeskOutcome::Action(action) => return Some(action),
            DeskOutcome::SwitchWorkspace => self.choose(),
        }
        None
    }

    fn choose_key(&mut self, key: KeyEvent) -> Option<Action> {
        let Screen::Choose(chooser) = &mut self.screen else {
            return None;
        };
        if let Some((path, _)) = chooser.confirm.clone() {
            match key.code {
                KeyCode::Char('y') | KeyCode::Enter => self.create(&path),
                KeyCode::Char('n') | KeyCode::Esc => chooser.confirm = None,
                _ => {}
            }
            return None;
        }
        if let Some(input) = &mut chooser.path {
            match key.code {
                KeyCode::Enter => {
                    let path = expand_home(input.text());
                    chooser.path = None;
                    self.open_or_create(&path);
                }
                KeyCode::Esc => chooser.path = None,
                _ => {
                    input.handle(key);
                }
            }
            return None;
        }
        let rows = chooser.recent.len() + 1;
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Some(Action::Quit),
            KeyCode::Up | KeyCode::Char('k') => {
                chooser.selected = chooser.selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                chooser.selected = (chooser.selected + 1).min(rows - 1);
            }
            KeyCode::Char('o') => chooser.path = Some(TextInput::default()),
            KeyCode::Enter => match chooser.recent.get(chooser.selected).cloned() {
                Some(path) => self.open(&path),
                None => chooser.path = Some(TextInput::default()),
            },
            KeyCode::Char('x') | KeyCode::Delete => {
                if let Some(path) = chooser.recent.get(chooser.selected).cloned() {
                    match self.profile.forget_workspace(&path) {
                        Ok(()) => {
                            self.message = Some(Message::info(format!(
                                "Forgot {}; its files are untouched.",
                                path.display()
                            )));
                            self.choose();
                        }
                        Err(error) => self.message = Some(Message::error(error.to_string())),
                    }
                }
            }
            _ => {}
        }
        None
    }

    fn open_or_create(&mut self, path: &Path) {
        match ProfileSession::inspect(path) {
            Ok(DirectoryState::Workspace) => self.open(path),
            Ok(DirectoryState::Missing | DirectoryState::Empty) => self.create(path),
            Ok(state @ DirectoryState::NotEmpty) => {
                if let Screen::Choose(chooser) = &mut self.screen {
                    chooser.confirm = Some((path.to_owned(), state));
                }
            }
            Err(error) => self.message = Some(Message::error(error.to_string())),
        }
    }

    fn open(&mut self, path: &Path) {
        match self.profile.open_workspace(path, self.clock.clone()) {
            Ok(session) => self.screen = Screen::Workspace(Box::new(Desk::new(session))),
            Err(error) => self.message = Some(Message::error(error.to_string())),
        }
    }

    fn create(&mut self, path: &Path) {
        match self
            .profile
            .create_workspace(path, None, self.clock.clone())
        {
            Ok(session) => {
                self.message = Some(Message::info(format!(
                    "Created a workspace in {}. Press n to add a task.",
                    session.workspace().root().display()
                )));
                self.screen = Screen::Workspace(Box::new(Desk::new(session)));
            }
            Err(error) => {
                if let Screen::Choose(chooser) = &mut self.screen {
                    chooser.confirm = None;
                }
                self.message = Some(Message::error(error.to_string()));
            }
        }
    }
}

fn expand_home(text: &str) -> PathBuf {
    let text = text.trim();
    match (text.strip_prefix("~/"), std::env::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ if text == "~" => std::env::home_dir().unwrap_or_default(),
        _ => PathBuf::from(text),
    }
}

pub enum DeskOutcome {
    Stay(Option<Message>),
    Action(Action),
    SwitchWorkspace,
}

impl Desk {
    pub fn new(session: Session) -> Self {
        let (sender, events) = mpsc::channel();
        let watcher = session
            .watch(move |event| {
                let _ = sender.send(event);
            })
            .ok();
        let mut desk = Self {
            session,
            _watcher: watcher,
            events,
            nav: Vec::new(),
            nav_selected: 0,
            focus: Focus::List,
            layout: Layout::List,
            search: String::new(),
            rows: Vec::new(),
            selected: 0,
            problems: Vec::new(),
            problem_selected: 0,
            detail_id: None,
            field: Field::Title,
            mode: Mode::Normal,
        };
        desk.rebuild();
        desk
    }

    /// Takes in changes the watcher saw.
    fn poll(&mut self) -> Option<Message> {
        let mut reloaded = false;
        while let Ok(event) = self.events.try_recv() {
            match self.session.apply(&event) {
                Ok(changed) => reloaded |= changed,
                Err(error) => return Some(Message::error(error.to_string())),
            }
        }
        reloaded.then(|| {
            self.rebuild();
            Message::info("Loaded changes made outside witdo.")
        })
    }

    pub fn current_nav(&self) -> &Nav {
        &self.nav[self.nav_selected.min(self.nav.len() - 1)]
    }

    pub fn filter(&self) -> Filter {
        Filter {
            text: self.search.clone(),
            ..Filter::default()
        }
    }

    /// Recomputes the sidebar, rows and problems, keeping the selections where they were.
    pub fn rebuild(&mut self) {
        let current = self.nav.get(self.nav_selected).cloned();
        let mut nav: Vec<Nav> = [
            View::Inbox,
            View::Next,
            View::Due,
            View::Open,
            View::Completed,
            View::All,
        ]
        .into_iter()
        .map(Nav::View)
        .collect();
        let categories = self.session.tasks().categories();
        nav.extend(
            categories
                .into_iter()
                .map(|c| Nav::View(View::Category(Some(c)))),
        );
        nav.push(Nav::View(View::Category(None)));
        nav.push(Nav::Problems);
        self.nav_selected = current
            .and_then(|current| nav.iter().position(|item| *item == current))
            .unwrap_or(1); // Next: what to do now
        self.nav = nav;

        let selected_id = self.selected_id();
        self.rows = match self.current_nav().clone() {
            Nav::View(view) => self.session.rows(&view, &self.filter(), self.layout),
            Nav::Problems => Vec::new(),
        };
        self.selected = selected_id
            .and_then(|id| self.rows.iter().position(|row| row.id == id))
            .unwrap_or(self.selected)
            .min(self.rows.len().saturating_sub(1));
        self.problems = self.session.problems();
        self.problem_selected = self
            .problem_selected
            .min(self.problems.len().saturating_sub(1));
        if self
            .detail_id
            .is_some_and(|id| !self.session.tasks().contains(id))
        {
            self.detail_id = None;
            self.focus = Focus::List;
        }
    }

    pub fn selected_id(&self) -> Option<TaskId> {
        self.rows.get(self.selected).map(|row| row.id)
    }

    /// The task the detail panel shows.
    pub fn shown_id(&self) -> Option<TaskId> {
        self.detail_id.or_else(|| self.selected_id())
    }

    fn handle_key(&mut self, key: KeyEvent, clock: &Clock) -> DeskOutcome {
        let mode = std::mem::replace(&mut self.mode, Mode::Normal);
        let message = match mode {
            Mode::Normal => return self.normal_key(key, clock),
            Mode::Help => None,
            Mode::Capture(input) => self.capture_key(key, input),
            Mode::Search(input) => {
                self.search_key(key, input);
                None
            }
            Mode::EditField(field, input) => self.field_key(key, field, input, clock),
            Mode::Pick(picker) => self.pick_key(key, picker),
            Mode::Confirm(confirm) => self.confirm_key(key, confirm),
        };
        DeskOutcome::Stay(message)
    }

    fn normal_key(&mut self, key: KeyEvent, clock: &Clock) -> DeskOutcome {
        let ctrl_enter =
            key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::CONTROL);
        let message = match key.code {
            KeyCode::Char('q') => return DeskOutcome::Action(Action::Quit),
            KeyCode::Char('w') => return DeskOutcome::SwitchWorkspace,
            KeyCode::Char('?') => {
                self.mode = Mode::Help;
                None
            }
            KeyCode::Char('n') => {
                self.mode = Mode::Capture(TextInput::default());
                None
            }
            KeyCode::Char('/') => {
                self.mode = Mode::Search(TextInput::new(&self.search));
                None
            }
            KeyCode::Char('t') => {
                self.layout = match self.layout {
                    Layout::List => Layout::Tree,
                    Layout::Tree => Layout::List,
                };
                self.rebuild();
                None
            }
            KeyCode::Char('r') => self.reload(),
            _ if ctrl_enter => self.toggle_done(),
            KeyCode::Char('x' | ' ') if self.focus != Focus::Sidebar => self.toggle_done(),
            KeyCode::Char('d') | KeyCode::Delete if self.focus != Focus::Sidebar => {
                self.ask_delete();
                None
            }
            KeyCode::Char('e') if self.focus != Focus::Sidebar => return self.open_editor(),
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Focus::Sidebar => Focus::List,
                    Focus::List => self.enter_detail(),
                    Focus::Detail => self.leave_detail(),
                };
                None
            }
            KeyCode::BackTab => {
                self.focus = match self.focus {
                    Focus::Sidebar => self.enter_detail(),
                    Focus::List => Focus::Sidebar,
                    Focus::Detail => {
                        self.leave_detail();
                        Focus::List
                    }
                };
                None
            }
            KeyCode::Esc => {
                if self.focus == Focus::Detail {
                    self.focus = self.leave_detail();
                } else if !self.search.is_empty() {
                    self.search.clear();
                    self.rebuild();
                }
                None
            }
            _ => match self.focus {
                Focus::Sidebar => self.sidebar_key(key),
                Focus::List => self.list_key(key),
                Focus::Detail => self.detail_key(key, clock),
            },
        };
        DeskOutcome::Stay(message)
    }

    fn enter_detail(&mut self) -> Focus {
        if matches!(self.current_nav(), Nav::Problems) {
            return Focus::List;
        }
        match self.selected_id() {
            Some(id) => {
                self.detail_id = Some(id);
                Focus::Detail
            }
            None => Focus::List,
        }
    }

    fn leave_detail(&mut self) -> Focus {
        if let Some(id) = self.detail_id.take()
            && let Some(index) = self.rows.iter().position(|row| row.id == id)
        {
            self.selected = index;
        }
        Focus::List
    }

    fn sidebar_key(&mut self, key: KeyEvent) -> Option<Message> {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.nav_selected = self.nav_selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.nav_selected = (self.nav_selected + 1).min(self.nav.len() - 1);
            }
            KeyCode::Enter | KeyCode::Right => {
                self.focus = Focus::List;
                return None;
            }
            _ => return None,
        }
        self.selected = 0;
        self.rebuild();
        None
    }

    fn list_key(&mut self, key: KeyEvent) -> Option<Message> {
        let problems = matches!(self.current_nav(), Nav::Problems);
        let (selected, len) = if problems {
            (&mut self.problem_selected, self.problems.len())
        } else {
            (&mut self.selected, self.rows.len())
        };
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => *selected = selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => {
                *selected = (*selected + 1).min(len.saturating_sub(1));
            }
            KeyCode::Home | KeyCode::Char('g') => *selected = 0,
            KeyCode::End | KeyCode::Char('G') => *selected = len.saturating_sub(1),
            KeyCode::Left => self.focus = Focus::Sidebar,
            KeyCode::Enter | KeyCode::Right if !problems => self.focus = self.enter_detail(),
            KeyCode::Enter if problems => return self.jump_to_problem(),
            _ => {}
        }
        None
    }

    /// From a problem, shows the task it is about, if it loaded.
    fn jump_to_problem(&mut self) -> Option<Message> {
        let problem = self.problems.get(self.problem_selected)?;
        let Some(&id) = problem.tasks.first() else {
            return Some(Message::info(format!(
                "Fix {} in another editor; witdo reloads it when it changes.",
                problem.files.join(", ")
            )));
        };
        self.nav_selected = self
            .nav
            .iter()
            .position(|nav| *nav == Nav::View(View::All))
            .unwrap_or(0);
        self.search.clear();
        self.rebuild();
        self.selected = self.rows.iter().position(|row| row.id == id).unwrap_or(0);
        self.focus = self.enter_detail();
        None
    }

    fn detail_key(&mut self, key: KeyEvent, clock: &Clock) -> Option<Message> {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.field = self.field.step(false),
            KeyCode::Down | KeyCode::Char('j') => self.field = self.field.step(true),
            KeyCode::Left | KeyCode::Right
                if matches!(self.field, Field::Status | Field::Priority) =>
            {
                return self.cycle(self.field, key.code == KeyCode::Right, clock);
            }
            KeyCode::Enter => return self.edit_field(),
            _ => {}
        }
        None
    }

    fn edit_field(&mut self) -> Option<Message> {
        let detail = self.session.detail(self.shown_id()?)?;
        let task = &detail.task;
        let text = match self.field {
            Field::Title => task.title.to_string(),
            Field::Due => task
                .due_date
                .map(|date| date.to_string())
                .unwrap_or_default(),
            Field::Category => task
                .category
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default(),
            Field::Tags => format_tags(task.tags()),
            Field::Status | Field::Priority => {
                return Some(Message::info("Use ← and → to change it."));
            }
            Field::Parent => {
                self.open_picker(Purpose::Parent);
                return None;
            }
            Field::Prerequisites => {
                self.open_picker(Purpose::Prerequisite);
                return None;
            }
            Field::Notes => {
                return Some(Message::info("Press e to edit the notes in your editor."));
            }
        };
        self.mode = Mode::EditField(self.field, TextInput::new(&text));
        None
    }

    fn open_picker(&mut self, purpose: Purpose) {
        let mut picker = Picker {
            purpose,
            input: TextInput::default(),
            results: Vec::new(),
            selected: 0,
        };
        self.refresh_picker(&mut picker);
        self.mode = Mode::Pick(picker);
    }

    fn refresh_picker(&self, picker: &mut Picker) {
        picker.results = self
            .session
            .find_tasks(picker.input.text(), self.shown_id());
        let rows = picker.results.len() + usize::from(picker.purpose == Purpose::Parent);
        picker.selected = picker.selected.min(rows.saturating_sub(1));
    }

    fn capture_key(&mut self, key: KeyEvent, mut input: TextInput) -> Option<Message> {
        match key.code {
            KeyCode::Enter => match self.session.capture(input.text()) {
                Ok(id) => {
                    self.rebuild();
                    if let Some(index) = self.rows.iter().position(|row| row.id == id) {
                        self.selected = index;
                    }
                    self.mode = Mode::Capture(TextInput::default());
                    Some(Message::info(format!(
                        "Added “{}” to the inbox.",
                        input.text().trim()
                    )))
                }
                Err(error) => {
                    self.mode = Mode::Capture(input);
                    Some(Message::error(error.to_string()))
                }
            },
            KeyCode::Esc => None,
            _ => {
                input.handle(key);
                self.mode = Mode::Capture(input);
                None
            }
        }
    }

    fn search_key(&mut self, key: KeyEvent, mut input: TextInput) {
        match key.code {
            KeyCode::Enter => {}
            KeyCode::Esc => {
                self.search.clear();
                self.rebuild();
            }
            _ => {
                input.handle(key);
                input.text().clone_into(&mut self.search);
                self.selected = 0;
                self.rebuild();
                self.mode = Mode::Search(input);
            }
        }
    }

    fn field_key(
        &mut self,
        key: KeyEvent,
        field: Field,
        mut input: TextInput,
        clock: &Clock,
    ) -> Option<Message> {
        match key.code {
            KeyCode::Enter => {
                let result = self.commit_field(field, input.text(), clock);
                if let Err(message) = &result
                    && message.error
                    && !matches!(self.mode, Mode::Confirm(_))
                {
                    self.mode = Mode::EditField(field, input);
                }
                result.err()
            }
            KeyCode::Esc => None,
            _ => {
                input.handle(key);
                self.mode = Mode::EditField(field, input);
                None
            }
        }
    }

    fn commit_field(&mut self, field: Field, text: &str, clock: &Clock) -> Result<(), Message> {
        let id = self
            .shown_id()
            .ok_or_else(|| Message::error("No task is selected."))?;
        let mut draft = self
            .session
            .draft(id)
            .ok_or_else(|| Message::error("That task is gone."))?;
        let task = &mut draft.task;
        let invalid = |error: AppError| Message::error(error.to_string());
        match field {
            Field::Title => task.title = Title::new(text).map_err(|e| invalid(e.into()))?,
            Field::Due => task.due_date = parse_due(text, clock.today()).map_err(invalid)?,
            Field::Category => {
                task.category = if text.trim().is_empty() {
                    None
                } else {
                    Some(Category::new(text).map_err(|e| invalid(e.into()))?)
                };
            }
            Field::Tags => task.set_tags(parse_tags(text)),
            _ => return Ok(()),
        }
        self.save(draft)
    }

    fn cycle(&mut self, field: Field, forward: bool, clock: &Clock) -> Option<Message> {
        let id = self.shown_id()?;
        let mut draft = self.session.draft(id)?;
        let next = |index: usize, len: usize| {
            if forward {
                (index + 1) % len
            } else {
                (index + len - 1) % len
            }
        };
        match field {
            Field::Status => {
                let index = Status::ALL
                    .iter()
                    .position(|&s| s == draft.task.status)
                    .unwrap_or(0);
                draft
                    .task
                    .set_status(Status::ALL[next(index, Status::ALL.len())], clock.now());
            }
            Field::Priority => {
                let index = Priority::ALL
                    .iter()
                    .position(|&p| p == draft.task.priority)
                    .unwrap_or(0);
                draft.task.priority = Priority::ALL[next(index, Priority::ALL.len())];
            }
            _ => return None,
        }
        self.save(draft).err()
    }

    fn pick_key(&mut self, key: KeyEvent, mut picker: Picker) -> Option<Message> {
        let offset = usize::from(picker.purpose == Purpose::Parent);
        let rows = picker.results.len() + offset;
        match key.code {
            KeyCode::Esc => return None,
            KeyCode::Up => picker.selected = picker.selected.saturating_sub(1),
            KeyCode::Down => picker.selected = (picker.selected + 1).min(rows.saturating_sub(1)),
            KeyCode::Enter => {
                let chosen = picker
                    .selected
                    .checked_sub(offset)
                    .and_then(|index| picker.results.get(index))
                    .map(|linked| linked.id);
                let id = self.shown_id()?;
                let mut draft = self.session.draft(id)?;
                match picker.purpose {
                    Purpose::Parent => {
                        draft.task.parent_id = chosen;
                        return self.save(draft).err();
                    }
                    Purpose::Prerequisite => {
                        let chosen = chosen?;
                        let prerequisites = &mut draft.task.depends_on;
                        if let Some(index) = prerequisites.iter().position(|&p| p == chosen) {
                            prerequisites.remove(index);
                        } else {
                            prerequisites.push(chosen);
                        }
                        let result = self.save(draft);
                        if !matches!(self.mode, Mode::Confirm(_)) {
                            self.mode = Mode::Pick(picker);
                        }
                        return result.err();
                    }
                }
            }
            _ => {
                if picker.input.handle(key) {
                    picker.selected = 0;
                    self.refresh_picker(&mut picker);
                }
            }
        }
        self.mode = Mode::Pick(picker);
        None
    }

    fn toggle_done(&mut self) -> Option<Message> {
        let id = self.shown_id()?;
        let task = self.session.tasks().get(id)?;
        if task.is_open() {
            let warnings = self.session.completion_warnings(id);
            if warnings.is_empty() {
                let result = self.session.complete(id);
                return self.finish(result, "Done.");
            }
            self.mode = Mode::Confirm(Confirm::Complete { id, warnings });
            None
        } else {
            {
                let result = self.session.reopen(id);
                self.finish(result, "Reopened.")
            }
        }
    }

    fn ask_delete(&mut self) {
        if let Some(id) = self.shown_id() {
            self.mode = Mode::Confirm(Confirm::Delete {
                id,
                choice: SubtaskChoice::default(),
            });
        }
    }

    fn confirm_key(&mut self, key: KeyEvent, confirm: Confirm) -> Option<Message> {
        let yes = matches!(key.code, KeyCode::Enter | KeyCode::Char('y'));
        let no = matches!(key.code, KeyCode::Esc | KeyCode::Char('n'));
        match confirm {
            Confirm::Complete { id, .. } if yes => {
                let result = self.session.complete(id);
                self.finish(result, "Done.")
            }
            Confirm::Delete { id, choice } if yes => {
                self.delete(id, choice, Overwrite::IfUnchanged)
            }
            Confirm::Delete { id, choice } if matches!(key.code, KeyCode::Char('s')) => {
                let choice = match choice {
                    SubtaskChoice::MoveToTopLevel => SubtaskChoice::Delete,
                    SubtaskChoice::Delete => SubtaskChoice::MoveToTopLevel,
                };
                self.mode = Mode::Confirm(Confirm::Delete { id, choice });
                None
            }
            Confirm::Conflict { .. } if matches!(key.code, KeyCode::Char('r')) => {
                let message = self.reload();
                message.or_else(|| Some(Message::info("Reloaded; your change was discarded.")))
            }
            Confirm::Conflict { retry, .. } if matches!(key.code, KeyCode::Char('o')) => {
                match retry {
                    Retry::Save(draft) => {
                        let result = self.session.save(*draft, Overwrite::Anyway);
                        self.finish(result, "Overwritten.")
                    }
                    Retry::Delete(id, choice) => self.delete(id, choice, Overwrite::Anyway),
                }
            }
            _ if no => None,
            confirm => {
                self.mode = Mode::Confirm(confirm);
                None
            }
        }
    }

    fn delete(
        &mut self,
        id: TaskId,
        choice: SubtaskChoice,
        overwrite: Overwrite,
    ) -> Option<Message> {
        let title = self
            .session
            .tasks()
            .get(id)
            .map(|t| t.title.to_string())
            .unwrap_or_default();
        match self.session.delete(id, choice, overwrite) {
            Ok(deleted) => {
                self.rebuild();
                let count = deleted.trashed.len();
                Some(Message::info(if count == 1 {
                    format!("Moved “{title}” to the trash.")
                } else {
                    format!("Moved “{title}” and {} subtasks to the trash.", count - 1)
                }))
            }
            Err(AppError::Conflict { file, deleted }) => {
                self.mode = Mode::Confirm(Confirm::Conflict {
                    file,
                    deleted,
                    retry: Retry::Delete(id, choice),
                });
                None
            }
            Err(error) => {
                self.rebuild();
                Some(Message::error(error.to_string()))
            }
        }
    }

    /// Saves a draft, asking what to do on a conflict.
    fn save(&mut self, draft: Draft) -> Result<(), Message> {
        match self.session.save(draft.clone(), Overwrite::IfUnchanged) {
            Ok(()) => {
                self.rebuild();
                Ok(())
            }
            Err(AppError::Conflict { file, deleted }) => {
                self.mode = Mode::Confirm(Confirm::Conflict {
                    file,
                    deleted,
                    retry: Retry::Save(Box::new(draft)),
                });
                Err(Message::info("The file changed outside witdo."))
            }
            Err(error) => Err(Message::error(error.to_string())),
        }
    }

    fn finish(&mut self, result: Result<(), AppError>, done: &str) -> Option<Message> {
        match result {
            Ok(()) => {
                self.rebuild();
                Some(Message::info(done))
            }
            Err(AppError::Conflict { file, deleted }) => {
                let id = self.shown_id()?;
                let draft = self.session.draft(id)?;
                self.mode = Mode::Confirm(Confirm::Conflict {
                    file,
                    deleted,
                    retry: Retry::Save(Box::new(draft)),
                });
                None
            }
            Err(error) => Some(Message::error(error.to_string())),
        }
    }

    fn reload(&mut self) -> Option<Message> {
        let result = self.session.refresh();
        self.rebuild();
        result.err().map(|error| Message::error(error.to_string()))
    }

    fn open_editor(&self) -> DeskOutcome {
        let Some(name) = self.shown_id().and_then(|id| self.session.file_name(id)) else {
            return DeskOutcome::Stay(None);
        };
        DeskOutcome::Action(Action::OpenEditor(
            self.session.workspace().tasks_dir().join(name),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;
    use witdo_app::{Launch, Profiles, start};

    use super::*;

    struct Fixture {
        _dir: TempDir,
        app: App,
    }

    fn fixture() -> Fixture {
        let dir = TempDir::new().unwrap();
        let profiles = Profiles::at(dir.path().join("config"));
        let launch = Launch {
            workspace: Some(dir.path().join("w")),
            ..Launch::default()
        };
        let clock = Clock::fixed("2026-10-21T12:00:00Z".parse().unwrap());
        let (profile, opening) = start(&launch, profiles, &clock).unwrap();
        let mut app = App::new(profile, opening, clock);
        app.handle_key(KeyEvent::from(KeyCode::Char('y')));
        Fixture { _dir: dir, app }
    }

    impl Fixture {
        fn keys(&mut self, keys: &str) {
            for c in keys.chars() {
                self.app.handle_key(KeyEvent::from(KeyCode::Char(c)));
            }
        }

        fn key(&mut self, code: KeyCode) -> Option<Action> {
            self.app.handle_key(KeyEvent::from(code))
        }

        fn desk(&mut self) -> &mut Desk {
            match &mut self.app.screen {
                Screen::Workspace(desk) => desk,
                Screen::Choose(_) => panic!("no workspace open"),
            }
        }

        fn titles(&mut self) -> Vec<String> {
            self.desk()
                .rows
                .iter()
                .map(|row| row.title.clone())
                .collect()
        }

        fn capture(&mut self, title: &str) {
            self.keys("n");
            self.keys(title);
            self.key(KeyCode::Enter);
            self.key(KeyCode::Esc);
        }

        fn view(&mut self, view: &View) {
            let desk = self.desk();
            desk.nav_selected = desk
                .nav
                .iter()
                .position(|nav| *nav == Nav::View(view.clone()))
                .unwrap();
            desk.rebuild();
        }

        fn message(&self) -> String {
            self.app
                .message
                .as_ref()
                .map(|m| m.text.clone())
                .unwrap_or_default()
        }
    }

    #[test]
    fn creates_the_workspace_after_confirming() {
        let mut fixture = fixture();
        assert!(fixture.message().starts_with("Created a workspace"));
        assert_eq!(fixture.desk().current_nav(), &Nav::View(View::Next));
    }

    #[test]
    fn captures_tasks_into_the_inbox() {
        let mut fixture = fixture();
        fixture.keys("nCall the bank");
        fixture.key(KeyCode::Enter);
        assert_eq!(fixture.message(), "Added “Call the bank” to the inbox.");
        assert!(
            matches!(fixture.desk().mode, Mode::Capture(_)),
            "ready for the next one"
        );
        fixture.key(KeyCode::Esc);
        fixture.view(&View::Inbox);
        assert_eq!(fixture.titles(), ["Call the bank"]);
        let dir = fixture.desk().session.workspace().tasks_dir();
        assert_eq!(fs::read_dir(dir).unwrap().count(), 1);
    }

    #[test]
    fn edits_fields_from_the_detail_panel() {
        let mut fixture = fixture();
        fixture.capture("Call the bank");
        fixture.view(&View::Inbox);
        fixture.key(KeyCode::Enter); // into the detail panel, on Title
        fixture.key(KeyCode::Enter);
        fixture.keys(" again");
        fixture.key(KeyCode::Enter);
        fixture.key(KeyCode::Down); // Status
        fixture.key(KeyCode::Right); // Inbox → To do
        fixture.key(KeyCode::Down);
        fixture.key(KeyCode::Down); // Due
        fixture.key(KeyCode::Enter);
        fixture.keys("tomorrow");
        fixture.key(KeyCode::Enter);

        let desk = fixture.desk();
        let id = desk.detail_id.unwrap();
        let task = desk.session.tasks().get(id).unwrap();
        assert_eq!(task.title.as_str(), "Call the bank again");
        assert_eq!(task.status, Status::Todo);
        assert_eq!(task.due_date, Some(jiff::civil::date(2026, 10, 22)));
        assert!(
            desk.session
                .file_name(id)
                .unwrap()
                .starts_with("call-the-bank-again--")
        );
    }

    #[test]
    fn a_bad_value_keeps_the_field_open() {
        let mut fixture = fixture();
        fixture.capture("Call the bank");
        fixture.view(&View::Inbox);
        fixture.key(KeyCode::Enter);
        fixture.desk().field = Field::Due;
        fixture.key(KeyCode::Enter);
        fixture.keys("soon");
        fixture.key(KeyCode::Enter);
        assert!(fixture.message().contains("is not a date"));
        assert!(matches!(
            fixture.desk().mode,
            Mode::EditField(Field::Due, _)
        ));
    }

    #[test]
    fn completes_and_reopens() {
        let mut fixture = fixture();
        fixture.capture("Call the bank");
        fixture.view(&View::Inbox);
        fixture.keys("x");
        fixture.view(&View::Completed);
        assert_eq!(fixture.titles(), ["Call the bank"]);
        fixture.keys("x");
        assert_eq!(fixture.titles(), Vec::<String>::new());
        fixture.view(&View::Open);
        assert_eq!(fixture.titles(), ["Call the bank"]);
    }

    #[test]
    fn warns_before_completing_with_open_subtasks() {
        let mut fixture = fixture();
        fixture.capture("Move house");
        fixture.capture("Pack books");
        fixture.view(&View::Inbox);
        let desk = fixture.desk();
        let parent = desk
            .rows
            .iter()
            .find(|row| row.title == "Move house")
            .unwrap()
            .id;
        let child = desk
            .rows
            .iter()
            .find(|row| row.title == "Pack books")
            .unwrap()
            .id;
        let mut draft = desk.session.draft(child).unwrap();
        draft.task.parent_id = Some(parent);
        desk.session.save(draft, Overwrite::IfUnchanged).unwrap();
        desk.rebuild();
        desk.selected = desk.rows.iter().position(|row| row.id == parent).unwrap();

        fixture.keys("x");
        assert!(matches!(
            fixture.desk().mode,
            Mode::Confirm(Confirm::Complete { .. })
        ));
        fixture.keys("y");
        assert_eq!(
            fixture.desk().session.tasks().get(parent).unwrap().status,
            Status::Done
        );
        assert_eq!(
            fixture.desk().session.tasks().get(child).unwrap().status,
            Status::Inbox
        );
    }

    #[test]
    fn deletes_after_confirming() {
        let mut fixture = fixture();
        fixture.capture("Call the bank");
        fixture.view(&View::Inbox);
        fixture.keys("d");
        assert!(matches!(
            fixture.desk().mode,
            Mode::Confirm(Confirm::Delete { .. })
        ));
        fixture.key(KeyCode::Enter);
        assert_eq!(fixture.message(), "Moved “Call the bank” to the trash.");
        assert_eq!(fixture.titles(), Vec::<String>::new());
        let trash = fixture.desk().session.workspace().trash_dir();
        assert_eq!(fs::read_dir(trash).unwrap().count(), 1);
    }

    #[test]
    fn searches_as_you_type() {
        let mut fixture = fixture();
        fixture.capture("Call the bank");
        fixture.capture("Pack books");
        fixture.view(&View::Inbox);
        fixture.keys("/BANK");
        assert_eq!(fixture.titles(), ["Call the bank"]);
        fixture.key(KeyCode::Esc);
        assert_eq!(fixture.titles().len(), 2);
    }

    #[test]
    fn picks_a_prerequisite_by_title() {
        let mut fixture = fixture();
        fixture.capture("Book the van");
        fixture.capture("Load the van");
        fixture.view(&View::Inbox);
        let desk = fixture.desk();
        desk.selected = desk
            .rows
            .iter()
            .position(|row| row.title == "Load the van")
            .unwrap();
        fixture.key(KeyCode::Enter);
        fixture.desk().field = Field::Prerequisites;
        fixture.key(KeyCode::Enter);
        fixture.keys("book");
        fixture.key(KeyCode::Enter);
        fixture.key(KeyCode::Esc);
        let desk = fixture.desk();
        let id = desk.detail_id.unwrap();
        assert!(desk.session.tasks().is_blocked(id));
    }

    #[test]
    fn asks_about_conflicts() {
        let mut fixture = fixture();
        fixture.capture("Call the bank");
        fixture.view(&View::Inbox);
        let desk = fixture.desk();
        let id = desk.selected_id().unwrap();
        let path = desk
            .session
            .workspace()
            .tasks_dir()
            .join(desk.session.file_name(id).unwrap());
        fs::write(&path, fs::read_to_string(&path).unwrap() + "Theirs.\n").unwrap();

        fixture.keys("x");
        assert!(matches!(
            fixture.desk().mode,
            Mode::Confirm(Confirm::Conflict { .. })
        ));
        fixture.keys("r");
        let desk = fixture.desk();
        assert_eq!(desk.session.tasks().get(id).unwrap().body, "Theirs.\n");
        assert_eq!(desk.session.tasks().get(id).unwrap().status, Status::Inbox);
    }

    #[test]
    fn e_opens_the_task_file_in_the_editor() {
        let mut fixture = fixture();
        fixture.capture("Call the bank");
        fixture.view(&View::Inbox);
        let action = fixture.key(KeyCode::Char('e'));
        let Some(Action::OpenEditor(path)) = action else {
            panic!("{action:?}")
        };
        let desk = fixture.desk();
        let name = desk.session.file_name(desk.selected_id().unwrap()).unwrap();
        assert_eq!(path, desk.session.workspace().tasks_dir().join(name));
    }

    #[test]
    fn q_quits() {
        let mut fixture = fixture();
        assert_eq!(fixture.key(KeyCode::Char('q')), Some(Action::Quit));
    }
}
