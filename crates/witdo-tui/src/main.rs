//! witdo in the terminal: what was, what is, what comes next, and what to do.

use std::io;

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Layout};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph};
use ratatui::{DefaultTerminal, Frame};

fn main() -> io::Result<()> {
    ratatui::run(run)
}

fn run(terminal: &mut DefaultTerminal) -> io::Result<()> {
    loop {
        terminal.draw(draw)?;
        if let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
        {
            return Ok(());
        }
    }
}

fn draw(frame: &mut Frame) {
    let [body, footer] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());
    frame.render_widget(
        Paragraph::new("What was, what is, what comes next, and what to do.")
            .block(Block::bordered().title(" witdo ")),
        body,
    );
    frame.render_widget(Line::from("q quit"), footer);
}
