//! Drawing the terminal app.

use std::fmt::Write as _;

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout as Split, Rect};
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};
use witdo_app::model::{PrerequisiteState, Priority, Status, SubtaskChoice, View};
use witdo_app::{Detail, Layout, ProblemKind, Row};

use crate::app::{App, Chooser, Confirm, Desk, Field, Focus, Message, Mode, Nav, Purpose, Screen};
use crate::input::TextInput;

pub fn draw(frame: &mut Frame, app: &App) {
    let [header, body, footer] = Split::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    match &app.screen {
        Screen::Choose(chooser) => {
            frame.render_widget(
                Line::from(vec![
                    " witdo ".bold().reversed(),
                    format!("  profile {}", app.profile.name()).dim(),
                ]),
                header,
            );
            draw_chooser(frame, body, chooser);
            draw_footer(frame, footer, app.message.as_ref(), chooser_hints(chooser));
            if let Some(input) = &chooser.path {
                draw_input(frame, footer, "Folder: ", input);
            }
        }
        Screen::Workspace(desk) => {
            frame.render_widget(
                Line::from(vec![
                    " witdo ".bold().reversed(),
                    format!("  {}", desk.session.workspace().name()).bold(),
                    format!("  profile {}", app.profile.name()).dim(),
                ]),
                header,
            );
            draw_desk(frame, body, desk);
            draw_footer(frame, footer, app.message.as_ref(), desk_hints(desk));
            match &desk.mode {
                Mode::Capture(input) => draw_input(frame, footer, "New task: ", input),
                Mode::Search(input) => draw_input(frame, footer, "Search: ", input),
                Mode::EditField(field, input) => {
                    draw_input(frame, footer, &format!("{}: ", field.label()), input);
                }
                Mode::Pick(picker) => draw_picker(frame, body, desk, picker),
                Mode::Confirm(confirm) => draw_confirm(frame, body, desk, confirm),
                Mode::Help => draw_help(frame, body),
                Mode::Normal => {}
            }
        }
    }
}

fn chooser_hints(chooser: &Chooser) -> &'static str {
    if chooser.confirm.is_some() {
        "y make it a workspace · n cancel"
    } else if chooser.path.is_some() {
        "Enter open or create · Esc cancel"
    } else {
        "↑↓ choose · Enter open · o open a folder · x forget · q quit"
    }
}

fn desk_hints(desk: &Desk) -> &'static str {
    match (&desk.mode, desk.focus) {
        (Mode::Capture(_), _) => "Enter add · Esc done",
        (Mode::Search(_), _) => "Enter keep · Esc clear",
        (Mode::EditField(..), _) => "Enter save · Esc cancel",
        (Mode::Pick(_), _) => "type to search · ↑↓ choose · Enter pick · Esc close",
        (Mode::Confirm(_) | Mode::Help, _) => "",
        (Mode::Normal, Focus::Detail) => {
            "↑↓ field · Enter edit · ←→ change · e notes · x done · Esc back"
        }
        (Mode::Normal, _) => {
            "n new · / search · x done · d delete · t tree · Tab panels · ? help · q quit"
        }
    }
}

fn draw_footer(frame: &mut Frame, area: Rect, message: Option<&Message>, hints: &str) {
    let line = match message {
        Some(message) if message.error => Line::from(message.text.clone().red()),
        Some(message) => Line::from(message.text.clone().green()),
        None => Line::from(hints.dim()),
    };
    frame.render_widget(line, area);
}

fn draw_input(frame: &mut Frame, area: Rect, label: &str, input: &TextInput) {
    frame.render_widget(Clear, area);
    frame.render_widget(Line::from(vec![label.bold(), input.text().into()]), area);
    let before: String = input.text().chars().take(input.cursor()).collect();
    let x =
        area.x + u16::try_from(label.chars().count() + before.chars().count()).unwrap_or(u16::MAX);
    frame.set_cursor_position((x.min(area.right().saturating_sub(1)), area.y));
}

fn draw_chooser(frame: &mut Frame, area: Rect, chooser: &Chooser) {
    let area = centered(area, 72, 20);
    let mut items: Vec<ListItem> = chooser
        .recent
        .iter()
        .map(|path| ListItem::new(path.display().to_string()))
        .collect();
    items.push(ListItem::new(
        "Open or create a workspace in another folder…".italic(),
    ));
    let list = List::new(items)
        .block(Block::bordered().title(" Choose a workspace "))
        .highlight_style(Style::new().reversed());
    let mut state = ListState::default().with_selected(Some(chooser.selected));
    frame.render_stateful_widget(list, area, &mut state);

    if let Some((path, state)) = &chooser.confirm {
        let text = match state {
            witdo_app::DirectoryState::NotEmpty => format!(
                "{} already has files in it.\n\nMake it a witdo workspace? witdo adds tasks/ and \
                 .witdo/ next to them and never moves, renames or changes them.\n\ny yes · n no",
                path.display()
            ),
            _ => format!("Make {} a witdo workspace?\n\ny yes · n no", path.display()),
        };
        draw_dialog(frame, area, " New workspace ", Text::from(text));
    }
}

fn draw_desk(frame: &mut Frame, area: Rect, desk: &Desk) {
    let [sidebar, list, detail] = Split::horizontal([
        Constraint::Length(26),
        Constraint::Fill(3),
        Constraint::Fill(2),
    ])
    .areas(area);
    draw_sidebar(frame, sidebar, desk);
    if matches!(desk.current_nav(), Nav::Problems) {
        draw_problems(frame, list, detail, desk);
    } else {
        draw_rows(frame, list, desk);
        draw_detail(frame, detail, desk);
    }
}

fn focused(block: Block<'_>, focus: bool) -> Block<'_> {
    if focus {
        block.border_style(Style::new().bold())
    } else {
        block.border_style(Style::new().dim())
    }
}

fn draw_sidebar(frame: &mut Frame, area: Rect, desk: &Desk) {
    let counts = desk.session.view_counts();
    let today = desk.session.clock().today();
    let items: Vec<ListItem> = desk
        .nav
        .iter()
        .map(|nav| {
            let (label, count) = match nav {
                Nav::View(View::Inbox) => ("Inbox".to_owned(), counts.inbox),
                Nav::View(View::Next) => ("Next".to_owned(), counts.next),
                Nav::View(View::Due) => ("Today / overdue".to_owned(), counts.due),
                Nav::View(View::Open) => ("Open".to_owned(), counts.open),
                Nav::View(View::Completed) => ("Completed".to_owned(), counts.completed),
                Nav::View(View::All) => ("All".to_owned(), counts.all),
                Nav::View(view @ View::Category(category)) => (
                    category
                        .as_ref()
                        .map_or_else(|| "Uncategorized".to_owned(), |c| format!("# {c}")),
                    desk.session
                        .tasks()
                        .list(view, &witdo_app::model::Filter::default(), today)
                        .len(),
                ),
                Nav::Problems => ("Problems".to_owned(), desk.problems.len()),
            };
            let line = Line::from(vec![
                Span::raw(format!("{label:<19}")),
                Span::raw(format!("{count:>4}")).dim(),
            ]);
            let item = ListItem::new(line);
            if matches!(nav, Nav::Problems) && count > 0 {
                item.yellow()
            } else {
                item
            }
        })
        .collect();
    let list = List::new(items)
        .block(focused(
            Block::bordered().title(" Views "),
            desk.focus == Focus::Sidebar,
        ))
        .highlight_style(Style::new().reversed());
    let mut state = ListState::default().with_selected(Some(desk.nav_selected));
    frame.render_stateful_widget(list, area, &mut state);
}

fn status_mark(status: Status) -> &'static str {
    match status {
        Status::Inbox => "•",
        Status::Todo => "○",
        Status::InProgress => "◐",
        Status::Done => "✓",
        Status::Cancelled => "✗",
    }
}

fn priority_mark(priority: Priority) -> Span<'static> {
    match priority {
        Priority::Urgent => "!!!".red().bold(),
        Priority::High => "!! ".yellow(),
        Priority::Normal => "   ".into(),
        Priority::Low => " ↓ ".dim(),
    }
}

fn row_line(row: &Row) -> Line<'static> {
    let mut spans = vec![
        Span::raw("  ".repeat(row.depth)),
        Span::raw(format!("{} ", status_mark(row.status))),
        priority_mark(row.priority),
        Span::raw(" "),
    ];
    let title = Span::raw(row.title.clone());
    spans.push(if row.status.is_open() {
        title
    } else {
        title.crossed_out()
    });
    if row.blocked {
        spans.push(" ⏸ blocked".yellow());
    }
    if row.prerequisite_cancelled {
        spans.push(" ⚠ prerequisite cancelled".yellow());
    }
    if let Some(due) = row.due {
        let due = Span::raw(format!("  {due}"));
        spans.push(if row.overdue { due.red() } else { due.dim() });
    }
    if let Some(category) = &row.category {
        spans.push(format!("  #{category}").cyan());
    }
    let line = Line::from(spans);
    if row.context { line.dim() } else { line }
}

fn draw_rows(frame: &mut Frame, area: Rect, desk: &Desk) {
    let title = match (desk.current_nav(), desk.layout) {
        (Nav::View(view), layout) => {
            let name = match view {
                View::Inbox => "Inbox".to_owned(),
                View::Next => "Next".to_owned(),
                View::Due => "Today / overdue".to_owned(),
                View::Open => "Open".to_owned(),
                View::Completed => "Completed".to_owned(),
                View::All => "All".to_owned(),
                View::Category(Some(category)) => format!("# {category}"),
                View::Category(None) => "Uncategorized".to_owned(),
            };
            let layout = if layout == Layout::Tree {
                " · tree"
            } else {
                ""
            };
            let search = if desk.search.is_empty() {
                String::new()
            } else {
                format!(" · “{}”", desk.search)
            };
            format!(" {name}{layout}{search} ")
        }
        (Nav::Problems, _) => " Problems ".to_owned(),
    };
    let block = focused(Block::bordered().title(title), desk.focus == Focus::List);
    if desk.rows.is_empty() {
        let empty = if desk.search.is_empty() {
            "Nothing here. Press n to add a task."
        } else {
            "No task matches the search."
        };
        frame.render_widget(Paragraph::new(empty.dim()).block(block), area);
        return;
    }
    let items: Vec<ListItem> = desk
        .rows
        .iter()
        .map(|row| ListItem::new(row_line(row)))
        .collect();
    let highlight = if desk.focus == Focus::List {
        Style::new().reversed()
    } else {
        Style::new().add_modifier(Modifier::UNDERLINED)
    };
    let list = List::new(items).block(block).highlight_style(highlight);
    let mut state = ListState::default().with_selected(Some(desk.selected));
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_detail(frame: &mut Frame, area: Rect, desk: &Desk) {
    let block = focused(
        Block::bordered().title(" Task "),
        desk.focus == Focus::Detail,
    );
    let Some(detail) = desk.shown_id().and_then(|id| desk.session.detail(id)) else {
        frame.render_widget(Paragraph::new("No task selected.".dim()).block(block), area);
        return;
    };
    let lines = detail_lines(&detail, desk);
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

#[expect(
    clippy::too_many_lines,
    reason = "one block per field, in the order shown"
)]
fn detail_lines(detail: &Detail, desk: &Desk) -> Vec<Line<'static>> {
    let task = &detail.task;
    let editing = desk.focus == Focus::Detail;
    let field = |field: Field, value: Line<'static>| {
        let label = format!("{:<11}", field.label());
        let label = if editing && desk.field == field {
            Span::raw(label).reversed()
        } else {
            Span::raw(label).dim()
        };
        let mut spans = vec![label, Span::raw(" ")];
        spans.extend(value.spans);
        Line::from(spans)
    };
    let none = || Line::from("—".dim());
    let mut lines = vec![
        field(Field::Title, Line::from(task.title.to_string().bold())),
        field(
            Field::Status,
            Line::from(format!(
                "{} {}",
                status_mark(task.status),
                task.status.label()
            )),
        ),
        field(Field::Priority, Line::from(task.priority.label())),
        field(
            Field::Due,
            task.due_date
                .map_or_else(none, |due| Line::from(due.to_string())),
        ),
        field(
            Field::Category,
            task.category
                .as_ref()
                .map_or_else(none, |c| Line::from(c.to_string())),
        ),
        field(
            Field::Tags,
            if task.tags().is_empty() {
                none()
            } else {
                Line::from(witdo_app::format_tags(task.tags()))
            },
        ),
        field(
            Field::Parent,
            detail
                .parent
                .as_ref()
                .map_or_else(none, |parent| Line::from(parent.title.clone())),
        ),
    ];
    if detail.prerequisites.is_empty() {
        lines.push(field(Field::Prerequisites, none()));
    }
    for (index, (prerequisite, state)) in detail.prerequisites.iter().enumerate() {
        let state = match state {
            PrerequisiteState::Open => "open".yellow(),
            PrerequisiteState::Done => "done".green(),
            PrerequisiteState::Cancelled => "cancelled — check it".yellow(),
            PrerequisiteState::Missing => "missing".red(),
        };
        let value = Line::from(vec![
            Span::raw(format!("{} (", prerequisite.title)),
            state,
            Span::raw(")"),
        ]);
        if index == 0 {
            lines.push(field(Field::Prerequisites, value));
        } else {
            let mut spans = vec![Span::raw(" ".repeat(12))];
            spans.extend(value.spans);
            lines.push(Line::from(spans));
        }
    }
    let list = |label: &str, linked: &[witdo_app::Linked]| {
        let names: Vec<String> = linked.iter().map(|l| l.title.clone()).collect();
        Line::from(vec![
            Span::raw(format!("{label:<11} ")).dim(),
            Span::raw(if names.is_empty() {
                "—".to_owned()
            } else {
                names.join(", ")
            }),
        ])
    };
    lines.push(list("Blocks", &detail.dependents));
    lines.push(list("Subtasks", &detail.subtasks));
    if detail.on_parent_cycle {
        lines.push(Line::from(
            "⚠ Part of a loop of parents: see Problems.".yellow(),
        ));
    }
    if detail.on_dependency_cycle {
        lines.push(Line::from(
            "⚠ Part of a loop of tasks waiting for each other.".yellow(),
        ));
    }
    lines.push(field(
        Field::Notes,
        Line::from(if task.body.trim().is_empty() {
            "—".dim()
        } else {
            "".into()
        }),
    ));
    lines.extend(task.body.lines().map(|line| Line::from(line.to_owned())));
    lines.push(Line::default());
    lines.push(Line::from(detail.file_name.clone().dim()));
    lines
}

fn draw_problems(frame: &mut Frame, list_area: Rect, detail_area: Rect, desk: &Desk) {
    let block = focused(
        Block::bordered().title(" Problems "),
        desk.focus == Focus::List,
    );
    if desk.problems.is_empty() {
        frame.render_widget(
            Paragraph::new("No problems. All files loaded.".green()).block(block),
            list_area,
        );
    } else {
        let items: Vec<ListItem> = desk
            .problems
            .iter()
            .map(|problem| {
                let mark = if problem.kind == ProblemKind::Ignored {
                    "·".dim()
                } else {
                    "⚠".yellow()
                };
                let what = problem.files.first().cloned().unwrap_or_default();
                ListItem::new(Line::from(vec![mark, Span::raw(format!(" {what}"))]))
            })
            .collect();
        let list = List::new(items)
            .block(block)
            .highlight_style(Style::new().reversed());
        let mut state = ListState::default().with_selected(Some(desk.problem_selected));
        frame.render_stateful_widget(list, list_area, &mut state);
    }
    let block = Block::bordered()
        .title(" Problem ")
        .border_style(Style::new().dim());
    let text = desk
        .problems
        .get(desk.problem_selected)
        .map_or_else(Text::default, |problem| {
            let mut lines = vec![Line::from(problem.message.clone())];
            lines.push(Line::default());
            for file in &problem.files {
                let line = problem
                    .line
                    .map(|line| format!(", line {line}"))
                    .unwrap_or_default();
                lines.push(Line::from(format!("tasks/{file}{line}").dim()));
            }
            lines.push(Line::default());
            lines.push(Line::from(
                if problem.tasks.is_empty() {
                    "witdo leaves the file alone: fix it in another editor."
                } else {
                    "Enter shows the task."
                }
                .dim(),
            ));
            Text::from(lines)
        });
    frame.render_widget(
        Paragraph::new(text).block(block).wrap(Wrap { trim: false }),
        detail_area,
    );
}

fn draw_picker(frame: &mut Frame, area: Rect, desk: &Desk, picker: &crate::app::Picker) {
    let area = centered(area, 60, 16);
    frame.render_widget(Clear, area);
    let current = desk.shown_id().and_then(|id| desk.session.tasks().get(id));
    let title = match picker.purpose {
        Purpose::Parent => " Choose the parent ",
        Purpose::Prerequisite => " Blocked by (Enter adds or removes) ",
    };
    let block = Block::bordered().title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [search, results] =
        Split::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(inner);
    draw_input(frame, search, "Search: ", &picker.input);
    let mut items = Vec::new();
    if picker.purpose == Purpose::Parent {
        items.push(ListItem::new("(no parent: top level)".italic()));
    }
    for linked in &picker.results {
        let chosen = current.is_some_and(|task| match picker.purpose {
            Purpose::Parent => task.parent_id == Some(linked.id),
            Purpose::Prerequisite => task.depends_on.contains(&linked.id),
        });
        let mark = if chosen { "✓ " } else { "  " };
        let status = linked.status.map_or("", status_mark);
        items.push(ListItem::new(format!("{mark}{status} {}", linked.title)));
    }
    let list = List::new(items).highlight_style(Style::new().reversed());
    let mut state = ListState::default().with_selected(Some(picker.selected));
    frame.render_stateful_widget(list, results, &mut state);
}

fn draw_confirm(frame: &mut Frame, area: Rect, desk: &Desk, confirm: &Confirm) {
    let title = |id| {
        desk.session
            .tasks()
            .get(id)
            .map(|task| format!("“{}”", task.title))
            .unwrap_or_default()
    };
    let titles = |ids: &[witdo_app::model::TaskId]| {
        ids.iter()
            .map(|&id| title(id))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let (heading, text) = match confirm {
        Confirm::Complete { id, warnings } => {
            let mut text = format!("{} still has unfinished work:\n", title(*id));
            if !warnings.open_prerequisites.is_empty() {
                let _ = write!(
                    text,
                    "\nBlocked by: {}",
                    titles(&warnings.open_prerequisites)
                );
            }
            if !warnings.open_subtasks.is_empty() {
                let _ = write!(text, "\nOpen subtasks: {}", titles(&warnings.open_subtasks));
            }
            text.push_str("\n\nMark it done anyway? Other tasks stay as they are.\n\ny yes · n no");
            (" Complete ", text)
        }
        Confirm::Delete { id, choice } => {
            let plan = desk.session.deletion_plan(*id, *choice);
            let subtasks = desk.session.tasks().children(*id).len();
            let mut text = format!("Delete {}? Its file moves to .witdo/trash/.\n", title(*id));
            if subtasks > 0 {
                let subtasks_line = match choice {
                    SubtaskChoice::MoveToTopLevel => format!(
                        "\nIts {subtasks} subtask(s) move to the top level. (s: delete them too)"
                    ),
                    SubtaskChoice::Delete => format!(
                        "\n{} task(s) under it are deleted too. (s: keep them at the top level)",
                        plan.delete.len() - 1
                    ),
                };
                text.push_str(&subtasks_line);
            }
            if !plan.unlink.is_empty() {
                let _ = write!(
                    text,
                    "\n\nThese tasks wait for it and will no longer: {}",
                    titles(&plan.unlink)
                );
            }
            text.push_str("\n\nEnter delete · Esc cancel");
            (" Delete ", text)
        }
        Confirm::Conflict { file, deleted, .. } => (
            " Changed outside witdo ",
            format!(
                "{file} was {} outside witdo since it was loaded.\n\n\
                 r reload it (your change is discarded)\no overwrite it with your change\nEsc cancel",
                if *deleted { "deleted" } else { "changed" }
            ),
        ),
    };
    draw_dialog(frame, area, heading, Text::from(text));
}

fn draw_help(frame: &mut Frame, area: Rect) {
    let text = "\
n            new task (into the inbox)
/            search titles and notes
↑↓ or j k    move
Enter        open the task / edit a field
Tab          next panel (views, list, task)
Esc          back, or clear the search
x, Space     mark done / reopen (also Ctrl+Enter)
d, Delete    delete (moves the file to the trash)
e            edit the task's file in $EDITOR
t            list or tree
r            reload from disk
w            switch workspace
q            quit

In the task panel, ←→ change status and priority.
Due dates: YYYY-MM-DD, today or tomorrow.

Any key closes this.";
    draw_dialog(frame, area, " Keys ", Text::from(text));
}

fn draw_dialog(frame: &mut Frame, area: Rect, title: &str, text: Text<'_>) {
    let height = u16::try_from(text.lines.len() + 4).unwrap_or(u16::MAX);
    let area = centered(area, 64, height);
    frame.render_widget(Clear, area);
    let paragraph = Paragraph::new(text)
        .block(
            Block::bordered()
                .title(title.to_owned())
                .border_style(Style::new().bold()),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let [area] = Split::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(area);
    let [area] = Split::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);
    area
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent};
    use tempfile::TempDir;
    use witdo_app::{Clock, Launch, Profiles, start};

    use super::*;

    fn app(dir: &TempDir) -> App {
        let launch = Launch {
            workspace: Some(dir.path().join("w")),
            ..Launch::default()
        };
        let clock = Clock::fixed("2026-10-21T12:00:00Z".parse().unwrap());
        let (profile, opening) =
            start(&launch, Profiles::at(dir.path().join("config")), &clock).unwrap();
        let mut app = App::new(profile, opening, clock);
        app.handle_key(KeyEvent::from(KeyCode::Char('y')));
        app
    }

    fn keys(app: &mut App, keys: &str) {
        for c in keys.chars() {
            app.handle_key(KeyEvent::from(KeyCode::Char(c)));
        }
    }

    fn screen(app: &App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn draws_views_tasks_and_their_detail() {
        let dir = TempDir::new().unwrap();
        let mut app = app(&dir);
        keys(&mut app, "nCall the bank");
        app.handle_key(KeyEvent::from(KeyCode::Enter));
        keys(&mut app, "Book the van");
        app.handle_key(KeyEvent::from(KeyCode::Enter));
        app.handle_key(KeyEvent::from(KeyCode::Esc));
        app.handle_key(KeyEvent::from(KeyCode::BackTab));
        app.handle_key(KeyEvent::from(KeyCode::Up));
        let screen = screen(&app);
        if std::env::var_os("WITDO_SHOW_SCREEN").is_some() {
            println!("{screen}");
        }
        for expected in [
            "Inbox",
            "Next",
            "Problems",
            "Call the bank",
            "Book the van",
            "Title",
            "Blocked by",
        ] {
            assert!(
                screen.contains(expected),
                "{expected} missing from\n{screen}"
            );
        }
    }

    #[test]
    fn draws_the_help() {
        let dir = TempDir::new().unwrap();
        let mut app = app(&dir);
        keys(&mut app, "?");
        assert!(screen(&app).contains("mark done / reopen"));
    }
}
