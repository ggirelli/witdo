//! The desktop app's components. State and actions are in `model`.

use std::path::PathBuf;
use std::rc::Rc;

use dioxus::prelude::*;
use witdo_app::model::{
    Category, CategoryFilter, DueFilter, ParentFilter, PrerequisiteState, Priority, Status,
    SubtaskChoice, Tag, TaskId, Title, View,
};
use witdo_app::{
    DirectoryState, Layout, Linked, ProblemKind, Row, Theme, WatchEvent, Watcher, format_tags,
    parse_due, parse_tags,
};

use crate::model::{Dialog, Model, Nav, Resolution, Screen};

fn model() -> Signal<Model> {
    use_context::<Signal<Model>>()
}

fn focus(id: &str) {
    document::eval(&format!("document.getElementById('{id}')?.focus()"));
}

fn scroll_to_selected() {
    document::eval(
        "document.querySelector('.task-row.selected')?.scrollIntoView({block: 'nearest'})",
    );
}

/// Keys typed into a field stay there, except Escape, which leaves the field, and Ctrl
/// shortcuts (zoom, mark done), which the app handles.
fn field_keys(e: &KeyboardEvent) {
    if e.key() == Key::Escape {
        document::eval("document.activeElement?.blur()");
    } else if !e.modifiers().ctrl() {
        e.stop_propagation();
    }
}

/// Ctrl+= or Ctrl++ zooms in, Ctrl+- out, Ctrl+0 back to 100%, anywhere in the window.
fn zoom_keys(mut model: Signal<Model>, e: &KeyboardEvent) {
    if !e.modifiers().ctrl() {
        return;
    }
    let step = match e.key() {
        Key::Character(c) if c == "=" || c == "+" => Some(1),
        Key::Character(c) if c == "-" => Some(-1),
        Key::Character(c) if c == "0" => None,
        _ => return,
    };
    e.prevent_default();
    let zoom = model.write().zoom_by(step);
    dioxus::desktop::window().set_zoom_level(f64::from(zoom) / 100.0);
}

#[component]
pub fn App() -> Element {
    let model = model();
    let model_ref = model.read();
    let theme = model_ref.theme().map(|theme| match theme {
        Theme::Light => "light",
        Theme::Dark => "dark",
    });
    rsx! {
        div {
            class: "app",
            "data-theme": theme,
            onkeydown: move |e| zoom_keys(model, &e),
            {screen(&model_ref)}
            if model_ref.dialog.is_some() {
                DialogView {}
            }
            if let Some(message) = &model_ref.message {
                div {
                    class: if message.error { "toast error" } else { "toast" },
                    role: "status",
                    "{message.text}"
                }
            }
        }
    }
}

fn screen(model: &Model) -> Element {
    rsx! {
        match &model.screen {
            Screen::Choose { confirm } => rsx! { Chooser { confirm: confirm.clone() } },
            Screen::Desk(desk) => {
                let root = desk.session.workspace().root().display().to_string();
                rsx! { DeskView { key: "{root}" } }
            }
        }
    }
}

#[component]
fn Chooser(confirm: Option<(PathBuf, DirectoryState)>) -> Element {
    let mut model = model();
    let mut path = use_signal(String::new);
    let recent = model.read().profile.recent_workspaces().to_vec();
    let profile = model.read().profile.name().to_string();
    rsx! {
        main { class: "chooser",
            header { class: "chooser-head",
                h1 { "witdo" }
                p { class: "muted", "What was, what is, what then, do. · profile {profile}" }
            }
            if let Some((folder, state)) = confirm {
                section { class: "card",
                    h2 { "Make this folder a workspace?" }
                    p { "{folder.display()}" }
                    if state == DirectoryState::NotEmpty {
                        p { class: "muted",
                            "It already has files in it. witdo adds tasks/ and .witdo/ next to them and never moves, renames or changes them."
                        }
                    }
                    div { class: "buttons",
                        button {
                            class: "primary",
                            onclick: move |_| model.write().create(&folder),
                            "Make it a workspace"
                        }
                        button { onclick: move |_| model.write().cancel_confirm(), "Cancel" }
                    }
                }
            } else {
                section { class: "card",
                    h2 { "Recent workspaces" }
                    if recent.is_empty() {
                        p { class: "muted", "None yet. Choose a folder below to start." }
                    }
                    ul { class: "recent",
                        for folder in recent {
                            li { key: "{folder.display()}",
                                button {
                                    class: "link",
                                    onclick: {
                                        let folder = folder.clone();
                                        move |_| model.write().open(&folder)
                                    },
                                    "{folder.display()}"
                                }
                                button {
                                    class: "ghost small",
                                    title: "Forget this workspace (its files stay)",
                                    onclick: {
                                        let folder = folder.clone();
                                        move |_| model.write().forget(&folder)
                                    },
                                    "Forget"
                                }
                            }
                        }
                    }
                }
                form {
                    class: "card",
                    onsubmit: move |e| {
                        e.prevent_default();
                        let folder = expand_home(&path.read());
                        if !folder.as_os_str().is_empty() {
                            model.write().open_or_create(&folder);
                        }
                    },
                    h2 { "Open or create a workspace" }
                    label { r#for: "folder", "Folder" }
                    div { class: "buttons",
                        input {
                            id: "folder",
                            placeholder: "~/Documents/tasks",
                            value: "{path}",
                            oninput: move |e| path.set(e.value()),
                        }
                        button { class: "primary", r#type: "submit", "Open" }
                    }
                    p { class: "muted small",
                        "A folder that is not a workspace yet becomes one; if it has files in it, witdo asks first."
                    }
                }
            }
        }
    }
}

fn expand_home(text: &str) -> PathBuf {
    let text = text.trim();
    match (text.strip_prefix("~/"), std::env::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(text),
    }
}

#[component]
fn DeskView() -> Element {
    let mut model = model();
    // Watches the workspace while it is open; the watcher stops when this view goes away.
    let _watcher: Rc<Option<Watcher>> = use_hook(move || {
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel::<WatchEvent>();
        let watcher = model.read().desk().and_then(|desk| {
            desk.session
                .watch(move |event| {
                    let _ = sender.send(event);
                })
                .ok()
        });
        spawn(async move {
            while let Some(event) = events.recv().await {
                model.write().apply(&event);
            }
        });
        Rc::new(watcher)
    });

    let onkeydown = move |e: KeyboardEvent| {
        let ctrl = e.modifiers().ctrl();
        let selected = model.read().desk().and_then(|desk| desk.selected);
        match e.key() {
            Key::Character(c) if c == "n" && !ctrl => focus("capture"),
            Key::Character(c) if c == "/" && !ctrl => focus("search"),
            Key::ArrowDown => {
                model.write().step(true);
                scroll_to_selected();
            }
            Key::ArrowUp => {
                model.write().step(false);
                scroll_to_selected();
            }
            Key::Enter if ctrl => {
                if let Some(id) = selected {
                    model.write().toggle_done(id);
                }
            }
            Key::Enter => focus("title"),
            Key::Escape => {
                let mut model = model.write();
                if model.dialog.is_some() {
                    model.close_dialog();
                } else if model.desk().is_some_and(|desk| !desk.search.is_empty()) {
                    model.set_search(String::new());
                } else {
                    model.select(None);
                }
            }
            Key::Delete => {
                if let Some(id) = selected {
                    model.write().ask_delete(id);
                }
            }
            _ => return,
        }
        e.prevent_default();
    };

    let is_problems = model
        .read()
        .desk()
        .is_some_and(|desk| desk.nav == Nav::Problems);
    rsx! {
        div {
            class: "desk",
            tabindex: 0,
            onmounted: move |e| async move {
                let _ = e.set_focus(true).await;
            },
            onkeydown,
            Sidebar {}
            main { class: "main",
                Toolbar {}
                FilterBar {}
                if is_problems {
                    ProblemsList {}
                } else {
                    TaskList {}
                }
            }
            if !is_problems {
                DetailPanel {}
            }
        }
    }
}

#[component]
fn Sidebar() -> Element {
    let mut model = model();
    let model_ref = model.read();
    let Some(desk) = model_ref.desk() else {
        return rsx! {};
    };
    let counts = desk.session.view_counts();
    let today = desk.session.clock().today();
    let problems = desk.session.problems().len();
    let entries: Vec<(Nav, String, usize)> = vec![
        (Nav::View(View::Inbox), "Inbox".into(), counts.inbox),
        (Nav::View(View::Next), "Next".into(), counts.next),
        (Nav::View(View::Due), "Today / overdue".into(), counts.due),
        (Nav::View(View::Open), "Open".into(), counts.open),
        (
            Nav::View(View::Completed),
            "Completed".into(),
            counts.completed,
        ),
        (Nav::View(View::All), "All".into(), counts.all),
    ];
    let categories: Vec<(Nav, String, usize)> = desk
        .session
        .tasks()
        .categories()
        .into_iter()
        .map(Some)
        .chain([None])
        .map(|category| {
            let view = View::Category(category.clone());
            let count = desk
                .session
                .tasks()
                .list(&view, &witdo_app::model::Filter::default(), today)
                .len();
            let label = category.map_or_else(|| "Uncategorized".to_owned(), |c| c.to_string());
            (Nav::View(view), label, count)
        })
        .collect();
    let current = desk.nav.clone();
    let name = desk.session.workspace().name();
    let profile = model_ref.profile.name().to_string();
    rsx! {
        nav { class: "sidebar",
            div { class: "workspace",
                strong { "{name}" }
                span { class: "muted small", "profile {profile}" }
                button {
                    class: "ghost small",
                    onclick: move |_| model.write().switch_workspace(),
                    "Switch…"
                }
            }
            NavGroup { entries, current: current.clone() }
            h3 { "Categories" }
            NavGroup { entries: categories, current: current.clone() }
            div { class: "nav-bottom",
                NavGroup {
                    entries: vec![(Nav::Problems, "Problems".to_owned(), problems)],
                    current,
                    warn: problems > 0,
                }
                ThemeSwitch {}
            }
        }
    }
}

/// Follow the system, or pick light or dark.
#[component]
fn ThemeSwitch() -> Element {
    let mut model = model();
    let current = model.read().theme();
    rsx! {
        div { class: "theme", role: "group", aria_label: "Theme",
            for (theme, label) in [(None, "System"), (Some(Theme::Light), "Light"), (Some(Theme::Dark), "Dark")] {
                button {
                    key: "{label}",
                    class: if current == theme { "on" },
                    aria_pressed: current == theme,
                    onclick: move |_| model.write().set_theme(theme),
                    "{label}"
                }
            }
        }
    }
}

#[component]
fn NavGroup(
    entries: Vec<(Nav, String, usize)>,
    current: Nav,
    #[props(default)] warn: bool,
) -> Element {
    let mut model = model();
    rsx! {
        ul { class: "nav",
            for (nav, label, count) in entries {
                li { key: "{label}",
                    button {
                        class: if nav == current { "nav-item current" } else { "nav-item" },
                        class: if warn { "warn" },
                        onclick: {
                            let nav = nav.clone();
                            move |_| model.write().set_nav(nav.clone())
                        },
                        span { "{label}" }
                        span { class: "count", "{count}" }
                    }
                }
            }
        }
    }
}

#[component]
fn Toolbar() -> Element {
    let mut model = model();
    let mut title = use_signal(String::new);
    let (search, layout, active) = model
        .read()
        .desk()
        .map(|desk| (desk.search.clone(), desk.layout, desk.active_filters()))
        .unwrap_or_default();
    rsx! {
        div { class: "toolbar",
            form {
                class: "capture",
                onsubmit: move |e| {
                    e.prevent_default();
                    let text = title.read().clone();
                    if model.write().capture(&text) {
                        title.set(String::new());
                    }
                },
                input {
                    id: "capture",
                    placeholder: "Add a task to the inbox (n)",
                    value: "{title}",
                    oninput: move |e| title.set(e.value()),
                    onkeydown: move |e| field_keys(&e),
                }
            }
            input {
                id: "search",
                class: "search",
                r#type: "search",
                placeholder: "Search (/)",
                value: "{search}",
                oninput: move |e| model.write().set_search(e.value()),
                onkeydown: move |e| field_keys(&e),
            }
            button {
                class: if active > 0 { "on" },
                onclick: move |_| model.write().toggle_filters(),
                if active > 0 { "Filters ({active})" } else { "Filters" }
            }
            div { class: "segmented", role: "group", aria_label: "Layout",
                button {
                    class: if layout == Layout::List { "on" },
                    onclick: move |_| model.write().set_layout(Layout::List),
                    "List"
                }
                button {
                    class: if layout == Layout::Tree { "on" },
                    onclick: move |_| model.write().set_layout(Layout::Tree),
                    "Tree"
                }
            }
        }
    }
}

/// The filter value for "no category": categories are trimmed, so none starts with a space.
const NO_CATEGORY: &str = " uncategorized";

/// Narrows the list by status, priority, due date, category, tag and parent.
#[component]
fn FilterBar() -> Element {
    let mut model = model();
    let model_ref = model.read();
    let Some(desk) = model_ref.desk().filter(|desk| desk.show_filters) else {
        return rsx! {};
    };
    let filters = desk.filters.clone();
    let status = filters
        .statuses
        .first()
        .map(|s| s.as_str().to_owned())
        .unwrap_or_default();
    let priority = filters
        .priorities
        .first()
        .map(|p| p.as_str().to_owned())
        .unwrap_or_default();
    let due = match filters.due {
        None => "",
        Some(DueFilter::Overdue) => "overdue",
        Some(DueFilter::Today) => "today",
        Some(DueFilter::ThisWeek) => "week",
        Some(DueFilter::NoDate) => "none",
        Some(DueFilter::Between { .. }) => "range",
    };
    let category = match &filters.category {
        None => String::new(),
        Some(CategoryFilter::Uncategorized) => NO_CATEGORY.to_owned(),
        Some(CategoryFilter::Is(category)) => category.to_string(),
    };
    let tag = filters
        .tags
        .first()
        .map(|t| t.as_str().to_owned())
        .unwrap_or_default();
    let parent = match filters.parent {
        None => String::new(),
        Some(ParentFilter::TopLevel) => "top".to_owned(),
        Some(ParentFilter::SubtreeOf(id)) => id.to_string(),
    };
    let categories = desk.session.tasks().categories();
    let tags = desk.session.tasks().tags();
    let parents: Vec<Linked> = desk
        .session
        .find_tasks("", None)
        .into_iter()
        .filter(|task| !desk.session.tasks().children(task.id).is_empty())
        .collect();
    rsx! {
        div { class: "filters",
            label { "Status "
                select {
                    onchange: move |e| {
                        let value = e.value();
                        model.write().set_filters(|f| f.statuses = value.parse::<Status>().into_iter().collect());
                    },
                    option { value: "", selected: status.is_empty(), "Any" }
                    for s in Status::ALL {
                        option { value: "{s.as_str()}", selected: status == s.as_str(), "{s.label()}" }
                    }
                }
            }
            label { "Priority "
                select {
                    onchange: move |e| {
                        let value = e.value();
                        model.write().set_filters(|f| f.priorities = value.parse::<Priority>().into_iter().collect());
                    },
                    option { value: "", selected: priority.is_empty(), "Any" }
                    for p in Priority::ALL {
                        option { value: "{p.as_str()}", selected: priority == p.as_str(), "{p.label()}" }
                    }
                }
            }
            label { "Due "
                select {
                    onchange: move |e| {
                        let due = match e.value().as_str() {
                            "overdue" => Some(DueFilter::Overdue),
                            "today" => Some(DueFilter::Today),
                            "week" => Some(DueFilter::ThisWeek),
                            "none" => Some(DueFilter::NoDate),
                            _ => None,
                        };
                        model.write().set_filters(|f| f.due = due);
                    },
                    option { value: "", selected: due.is_empty(), "Any" }
                    option { value: "overdue", selected: due == "overdue", "Overdue" }
                    option { value: "today", selected: due == "today", "Today" }
                    option { value: "week", selected: due == "week", "This week" }
                    option { value: "none", selected: due == "none", "No date" }
                }
            }
            label { "Category "
                select {
                    onchange: move |e| {
                        let value = e.value();
                        let category = match value.as_str() {
                            "" => None,
                            NO_CATEGORY => Some(CategoryFilter::Uncategorized),
                            name => Category::new(name).ok().map(CategoryFilter::Is),
                        };
                        model.write().set_filters(|f| f.category = category);
                    },
                    option { value: "", selected: category.is_empty(), "Any" }
                    option { value: NO_CATEGORY, selected: category == NO_CATEGORY, "None" }
                    for c in categories {
                        option { value: "{c}", selected: category == c.as_str(), "{c}" }
                    }
                }
            }
            label { "Tag "
                select {
                    onchange: move |e| {
                        let value = e.value();
                        model.write().set_filters(|f| f.tags = Tag::new(&value).into_iter().collect());
                    },
                    option { value: "", selected: tag.is_empty(), "Any" }
                    for t in tags {
                        option { value: "{t}", selected: tag.to_lowercase() == t.key(), "{t}" }
                    }
                }
            }
            label { "Parent "
                select {
                    onchange: move |e| {
                        let value = e.value();
                        let parent = match value.as_str() {
                            "" => None,
                            "top" => Some(ParentFilter::TopLevel),
                            id => id.parse::<TaskId>().ok().map(ParentFilter::SubtreeOf),
                        };
                        model.write().set_filters(|f| f.parent = parent);
                    },
                    option { value: "", selected: parent.is_empty(), "Any" }
                    option { value: "top", selected: parent == "top", "Top level only" }
                    for p in parents {
                        option { value: "{p.id}", selected: parent == p.id.to_string(), "Under {p.title}" }
                    }
                }
            }
            button {
                class: "ghost small",
                onclick: move |_| model.write().set_filters(|f| *f = witdo_app::model::Filter::default()),
                "Clear"
            }
        }
    }
}

#[component]
fn TaskList() -> Element {
    let model = model();
    let model_ref = model.read();
    let Some(desk) = model_ref.desk() else {
        return rsx! {};
    };
    let rows = desk.rows();
    let selected = desk.selected;
    let searching = !desk.search.is_empty();
    rsx! {
        div { class: "list", role: "list",
            if rows.is_empty() {
                p { class: "empty",
                    if searching { "No task matches the search." } else { "Nothing here. Add a task above." }
                }
            }
            for row in rows {
                TaskRow { key: "{row.id}", selected: Some(row.id) == selected, row }
            }
        }
    }
}

fn priority_badge(priority: Priority) -> Option<(&'static str, &'static str)> {
    match priority {
        Priority::Urgent => Some(("badge urgent", "Urgent")),
        Priority::High => Some(("badge high", "High")),
        Priority::Normal => None,
        Priority::Low => Some(("badge low", "Low")),
    }
}

#[component]
fn TaskRow(row: Row, selected: bool) -> Element {
    let mut model = model();
    let id = row.id;
    let closed = !row.status.is_open();
    // 0.75rem, plus 1.25rem per level of nesting.
    let indent = format!("padding-left: calc(0.75rem + {} * 1.25rem)", row.depth);
    rsx! {
        div {
            class: "task-row",
            class: if selected { "selected" },
            class: if row.context { "context" },
            class: if closed { "closed" },
            role: "listitem",
            style: "{indent}",
            onclick: move |_| model.write().select(Some(id)),
            input {
                r#type: "checkbox",
                checked: closed,
                title: if closed { "Reopen" } else { "Mark done" },
                onclick: move |e| {
                    e.stop_propagation();
                    model.write().toggle_done(id);
                },
            }
            span { class: "status status-{row.status.as_str()}", title: "{row.status.label()}" }
            span { class: "title", "{row.title}" }
            if row.blocked {
                span { class: "badge blocked", "Blocked" }
            }
            if row.prerequisite_cancelled {
                span { class: "badge warn", title: "A prerequisite was cancelled", "Check prerequisite" }
            }
            if let Some((class, label)) = priority_badge(row.priority) {
                span { class: "{class}", "{label}" }
            }
            if let Some(due) = row.due {
                span { class: if row.overdue { "due overdue" } else { "due" }, "{due}" }
            }
            if let Some(category) = &row.category {
                span { class: "chip", "{category}" }
            }
        }
    }
}

#[component]
fn ProblemsList() -> Element {
    let mut model = model();
    let problems = model
        .read()
        .desk()
        .map(|desk| desk.session.problems())
        .unwrap_or_default();
    rsx! {
        div { class: "problems",
            if problems.is_empty() {
                p { class: "empty", "No problems: every file loaded." }
            }
            for (index, problem) in problems.into_iter().enumerate() {
                article {
                    key: "{index}",
                    class: if problem.kind == ProblemKind::Ignored { "problem note" } else { "problem" },
                    p { "{problem.message}" }
                    for file in &problem.files {
                        code { class: "small",
                            "tasks/{file}"
                            if let Some(line) = problem.line {
                                ", line {line}"
                            }
                        }
                    }
                    if let Some(&id) = problem.tasks.first() {
                        button {
                            class: "ghost small",
                            onclick: move |_| {
                                let mut model = model.write();
                                model.set_nav(Nav::View(View::All));
                                model.set_search(String::new());
                                model.select(Some(id));
                            },
                            "Show the task"
                        }
                    } else {
                        p { class: "muted small", "witdo leaves the file alone: fix it in another editor and it reloads." }
                    }
                }
            }
        }
    }
}

#[component]
fn DetailPanel() -> Element {
    let model = model();
    let model_ref = model.read();
    let Some(desk) = model_ref.desk() else {
        return rsx! {};
    };
    let Some(detail) = desk.selected.and_then(|id| desk.session.detail(id)) else {
        return rsx! {
            aside { class: "detail empty", p { "Select a task to see it here." } }
        };
    };
    let revision = desk.revision();
    let id = detail.task.id;
    let others: Vec<Linked> = desk.session.find_tasks("", Some(id));
    let categories: Vec<String> = desk
        .session
        .tasks()
        .categories()
        .iter()
        .map(ToString::to_string)
        .collect();
    rsx! {
        aside { class: "detail", key: "{id}-{revision}",
            Editor { detail, others, categories }
        }
    }
}

#[component]
fn Editor(detail: witdo_app::Detail, others: Vec<Linked>, categories: Vec<String>) -> Element {
    let mut model = model();
    let task = detail.task.clone();
    let id = task.id;
    let closed = !task.is_open();
    let due = task.due_date.map(|d| d.to_string()).unwrap_or_default();
    let category = task
        .category
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_default();
    let tags = format_tags(task.tags());
    let parent = task.parent_id.map(|p| p.to_string()).unwrap_or_default();
    let prerequisites: Vec<TaskId> = task.depends_on.clone();
    let addable: Vec<Linked> = others
        .iter()
        .filter(|o| !prerequisites.contains(&o.id))
        .cloned()
        .collect();
    rsx! {
        div { class: "editor",
            input {
                id: "title",
                class: "title-input",
                aria_label: "Title",
                value: "{task.title}",
                onchange: move |e| {
                    let text = e.value();
                    model.write().edit(move |task, _| {
                        task.title = Title::new(&text)?;
                        Ok(())
                    });
                },
                onkeydown: move |e| field_keys(&e),
            }
            div { class: "buttons",
                button {
                    class: "primary",
                    onclick: move |_| model.write().toggle_done(id),
                    if closed { "Reopen" } else { "Mark done" }
                }
                button { class: "danger", onclick: move |_| model.write().ask_delete(id), "Delete…" }
            }
            if detail.blocked {
                p { class: "notice", "Blocked: waiting for an open prerequisite." }
            }
            if detail.on_parent_cycle {
                p { class: "notice warn", "Part of a loop of parents, so it is shown at the top level. See Problems." }
            }
            if detail.on_dependency_cycle {
                p { class: "notice warn", "Part of a loop of tasks waiting for each other. See Problems." }
            }
            div { class: "fields",
                label { r#for: "status", "Status" }
                select {
                    id: "status",
                    onchange: move |e| {
                        let value = e.value();
                        model.write().edit(move |task, clock| {
                            if let Ok(status) = value.parse::<Status>() {
                                task.set_status(status, clock.now());
                            }
                            Ok(())
                        });
                    },
                    for status in Status::ALL {
                        option { value: "{status.as_str()}", selected: status == task.status, "{status.label()}" }
                    }
                }
                label { r#for: "priority", "Priority" }
                select {
                    id: "priority",
                    onchange: move |e| {
                        let value = e.value();
                        model.write().edit(move |task, _| {
                            if let Ok(priority) = value.parse::<Priority>() {
                                task.priority = priority;
                            }
                            Ok(())
                        });
                    },
                    for priority in Priority::ALL {
                        option { value: "{priority.as_str()}", selected: priority == task.priority, "{priority.label()}" }
                    }
                }
                label { r#for: "due", "Due" }
                input {
                    id: "due",
                    r#type: "date",
                    value: "{due}",
                    onchange: move |e| {
                        let text = e.value();
                        model.write().edit(move |task, clock| {
                            task.due_date = parse_due(&text, clock.today())?;
                            Ok(())
                        });
                    },
                    onkeydown: move |e| field_keys(&e),
                }
                label { r#for: "category", "Category" }
                input {
                    id: "category",
                    list: "categories",
                    value: "{category}",
                    placeholder: "None",
                    onchange: move |e| {
                        let text = e.value();
                        model.write().edit(move |task, _| {
                            task.category = if text.trim().is_empty() { None } else { Some(Category::new(&text)?) };
                            Ok(())
                        });
                    },
                    onkeydown: move |e| field_keys(&e),
                }
                datalist { id: "categories",
                    for category in categories {
                        option { value: "{category}" }
                    }
                }
                label { r#for: "tags", "Tags" }
                input {
                    id: "tags",
                    value: "{tags}",
                    placeholder: "e.g. writing, q4",
                    onchange: move |e| {
                        let text = e.value();
                        model.write().edit(move |task, _| {
                            task.set_tags(parse_tags(&text));
                            Ok(())
                        });
                    },
                    onkeydown: move |e| field_keys(&e),
                }
                label { r#for: "parent", "Parent" }
                select {
                    id: "parent",
                    onchange: move |e| {
                        let value = e.value();
                        model.write().edit(move |task, _| {
                            task.parent_id = value.parse::<TaskId>().ok();
                            Ok(())
                        });
                    },
                    option { value: "", selected: parent.is_empty(), "None (top level)" }
                    for other in others.iter().cloned() {
                        option {
                            value: "{other.id}",
                            selected: parent == other.id.to_string(),
                            "{other.title}"
                        }
                    }
                }
            }
            section { class: "links",
                h3 { "Blocked by" }
                ul {
                    for (prerequisite , state) in detail.prerequisites.iter().cloned() {
                        li { key: "{prerequisite.id}",
                            LinkButton { linked: prerequisite.clone() }
                            span { class: "state state-{state_class(state)}", "{state_label(state)}" }
                            button {
                                class: "ghost small",
                                title: "Remove this prerequisite",
                                onclick: move |_| {
                                    let gone = prerequisite.id;
                                    model.write().edit(move |task, _| {
                                        task.depends_on.retain(|p| *p != gone);
                                        Ok(())
                                    });
                                },
                                "Remove"
                            }
                        }
                    }
                }
                select {
                    aria_label: "Add a prerequisite",
                    onchange: move |e| {
                        let value = e.value();
                        model.write().edit(move |task, _| {
                            if let Ok(id) = value.parse::<TaskId>() {
                                task.depends_on.push(id);
                            }
                            Ok(())
                        });
                    },
                    option { value: "", selected: true, "Add a prerequisite…" }
                    for other in addable {
                        option { value: "{other.id}", "{other.title}" }
                    }
                }
                h3 { "Blocks" }
                LinkList { links: detail.dependents.clone() }
                h3 { "Subtasks" }
                LinkList { links: detail.subtasks.clone() }
            }
            label { r#for: "notes", "Notes" }
            textarea {
                id: "notes",
                rows: 10,
                value: "{task.body}",
                placeholder: "Plain text, saved to the task's Markdown file.",
                onfocus: move |_| model.write().begin_notes(),
                onblur: move |_| model.write().end_notes(),
                onchange: move |e| {
                    let text = e.value();
                    model.write().edit(move |task, _| {
                        task.body = text;
                        Ok(())
                    });
                },
                onkeydown: move |e| field_keys(&e),
            }
            p { class: "muted small file", "tasks/{detail.file_name}" }
        }
    }
}

fn state_class(state: PrerequisiteState) -> &'static str {
    match state {
        PrerequisiteState::Open => "open",
        PrerequisiteState::Done => "done",
        PrerequisiteState::Cancelled => "cancelled",
        PrerequisiteState::Missing => "missing",
    }
}

fn state_label(state: PrerequisiteState) -> &'static str {
    match state {
        PrerequisiteState::Open => "open",
        PrerequisiteState::Done => "done",
        PrerequisiteState::Cancelled => "cancelled: check it",
        PrerequisiteState::Missing => "missing",
    }
}

#[component]
fn LinkList(links: Vec<Linked>) -> Element {
    rsx! {
        if links.is_empty() {
            p { class: "muted small", "None" }
        }
        ul {
            for linked in links {
                li { key: "{linked.id}", LinkButton { linked: linked.clone() } }
            }
        }
    }
}

#[component]
fn LinkButton(linked: Linked) -> Element {
    let mut model = model();
    let id = linked.id;
    let missing = linked.status.is_none();
    rsx! {
        button {
            class: "link",
            disabled: missing,
            onclick: move |_| model.write().select(Some(id)),
            if let Some(status) = linked.status {
                span { class: "status status-{status.as_str()}", title: "{status.label()}" }
            }
            "{linked.title}"
        }
    }
}

#[component]
fn DialogView() -> Element {
    let mut model = model();
    let model_ref = model.read();
    let (Some(dialog), Some(desk)) = (&model_ref.dialog, model_ref.desk()) else {
        return rsx! {};
    };
    let title = |id: TaskId| {
        desk.session
            .tasks()
            .get(id)
            .map(|task| format!("“{}”", task.title))
            .unwrap_or_default()
    };
    let titles = |ids: &[TaskId]| {
        ids.iter()
            .map(|&id| title(id))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let body = match dialog {
        Dialog::Complete { id, warnings } => rsx! {
            h2 { "Mark {title(*id)} done?" }
            p { "It still has unfinished work:" }
            if !warnings.open_prerequisites.is_empty() {
                p { strong { "Blocked by: " } "{titles(&warnings.open_prerequisites)}" }
            }
            if !warnings.open_subtasks.is_empty() {
                p { strong { "Open subtasks: " } "{titles(&warnings.open_subtasks)}" }
            }
            p { class: "muted", "Other tasks stay as they are." }
            div { class: "buttons",
                button { class: "primary", onclick: move |_| model.write().confirm_complete(), "Mark done anyway" }
                button { onclick: move |_| model.write().close_dialog(), "Cancel" }
            }
        },
        Dialog::Delete { id, choice } => {
            let plan = desk.session.deletion_plan(*id, *choice);
            let subtasks = desk.session.tasks().children(*id).len();
            let choice = *choice;
            rsx! {
                h2 { "Delete {title(*id)}?" }
                p { "Its file moves to .witdo/trash/ in the workspace; nothing is deleted for good." }
                if subtasks > 0 {
                    fieldset {
                        legend { "It has {subtasks} subtask(s)" }
                        label {
                            input {
                                r#type: "radio",
                                name: "subtasks",
                                checked: choice == SubtaskChoice::MoveToTopLevel,
                                onchange: move |_| model.write().set_delete_choice(SubtaskChoice::MoveToTopLevel),
                            }
                            " Move them to the top level"
                        }
                        label {
                            input {
                                r#type: "radio",
                                name: "subtasks",
                                checked: choice == SubtaskChoice::Delete,
                                onchange: move |_| model.write().set_delete_choice(SubtaskChoice::Delete),
                            }
                            " Delete them too ({plan.delete.len() - 1} task(s))"
                        }
                    }
                }
                if !plan.unlink.is_empty() {
                    p { "These tasks wait for it and will no longer: {titles(&plan.unlink)}" }
                }
                div { class: "buttons",
                    button { class: "danger", onclick: move |_| model.write().confirm_delete(), "Delete" }
                    button { onclick: move |_| model.write().close_dialog(), "Cancel" }
                }
            }
        }
        Dialog::Conflict { file, deleted, .. } => rsx! {
            h2 { "Changed outside witdo" }
            p {
                "{file} was "
                if *deleted { "deleted" } else { "changed" }
                " outside witdo since it was loaded."
            }
            div { class: "buttons",
                button { onclick: move |_| model.write().resolve_conflict(Resolution::Reload), "Reload (discard my change)" }
                button { class: "danger", onclick: move |_| model.write().resolve_conflict(Resolution::Overwrite), "Overwrite" }
                button { onclick: move |_| model.write().close_dialog(), "Cancel" }
            }
        },
    };
    rsx! {
        div { class: "overlay", onclick: move |_| model.write().close_dialog(),
            section { class: "dialog card", role: "dialog", onclick: move |e| e.stop_propagation(), {body} }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use tempfile::TempDir;
    use witdo_app::{Clock, Launch, Profiles, start};

    use super::*;

    #[derive(Clone)]
    struct Handover(Rc<RefCell<Option<Model>>>);

    #[allow(non_snake_case)]
    fn Harness() -> Element {
        let handover = use_context::<Handover>();
        use_context_provider(move || Signal::new(handover.0.borrow_mut().take().expect("once")));
        rsx! { App {} }
    }

    /// A workspace with a few related tasks, the second one selected.
    fn model(dir: &TempDir) -> Model {
        let launch = Launch {
            workspace: Some(dir.path().join("Home")),
            ..Launch::default()
        };
        let clock = Clock::fixed("2026-10-21T12:00:00Z".parse().unwrap());
        let (profile, opening) =
            start(&launch, Profiles::at(dir.path().join("config")), &clock).unwrap();
        let mut model = Model::new(profile, opening, clock);
        model.create(&dir.path().join("Home"));
        for title in [
            "Move house",
            "Book the van",
            "Load the van",
            "Call the bank",
        ] {
            model.capture(title);
        }
        let desk = model.desk().unwrap();
        let id = |title: &str| {
            desk.session
                .tasks()
                .iter()
                .find(|t| t.title.as_str() == title)
                .unwrap()
                .id
        };
        let (house, book, load) = (id("Move house"), id("Book the van"), id("Load the van"));
        let edit =
            |model: &mut Model, task: TaskId, change: &dyn Fn(&mut witdo_app::model::Task)| {
                model.select(Some(task));
                model.edit(|task, _| {
                    change(task);
                    Ok(())
                });
            };
        edit(&mut model, book, &|t| {
            t.parent_id = Some(house);
            t.status = Status::Todo;
            t.priority = Priority::High;
            t.due_date = Some("2026-10-20".parse().unwrap());
        });
        edit(&mut model, load, &|t| {
            t.parent_id = Some(house);
            t.depends_on = vec![book];
            t.status = Status::Todo;
            t.category = Some(Category::new("Home").unwrap());
            t.body = "Heavy boxes first.\n".into();
        });
        edit(&mut model, house, &|t| t.status = Status::InProgress);
        model.set_nav(Nav::View(View::Open));
        model.set_layout(Layout::Tree);
        model.toggle_filters();
        model.select(Some(load));
        model
    }

    fn render(model: Model) -> String {
        let mut dom = VirtualDom::new(Harness)
            .with_root_context(Handover(Rc::new(RefCell::new(Some(model)))));
        dom.rebuild_in_place();
        dioxus_ssr::render(&dom)
    }

    /// The page as the window shows it, for looking at in a browser.
    fn page(body: &str) -> String {
        format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><style>{}</style></head><body><div id=\"main\">{body}</div></body></html>",
            include_str!("style.css")
        )
    }

    #[test]
    fn renders_the_workspace() {
        let dir = TempDir::new().unwrap();
        let html = render(model(&dir));
        assert!(html.contains("System"));
        if let Some(path) = std::env::var_os("WITDO_RENDER_TO") {
            std::fs::write(path, page(&html)).unwrap();
        }
        for expected in [
            "Move house",
            "Load the van",
            "Blocked",
            "Heavy boxes first.",
            "Today / overdue",
            "Problems",
            "Top level only",
            "Mark done",
        ] {
            assert!(html.contains(expected), "{expected} missing");
        }
    }

    #[test]
    fn renders_the_chooser() {
        let dir = TempDir::new().unwrap();
        let mut model = model(&dir);
        model.switch_workspace();
        let html = render(model);
        assert!(html.contains("Recent workspaces"));
        assert!(html.contains("Open or create a workspace"));
    }
}
