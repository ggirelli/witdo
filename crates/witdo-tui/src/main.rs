//! witdo in the terminal: what was, what is, what then, do.

mod app;
mod input;
mod ui;

use std::io;
use std::path::Path;
use std::process::{Command as Process, ExitCode, ExitStatus};
use std::time::Duration;

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event};
use witdo_app::{Clock, Command, Profiles, parse_args, start, usage};

use crate::app::{Action, App};

fn main() -> ExitCode {
    let launch = match parse_args(
        std::env::args_os().skip(1),
        std::env::var_os("WITDO_PROFILE"),
    ) {
        Ok(Command::Run(launch)) => launch,
        Ok(Command::Help) => {
            print!("{}", usage("witdo-tui"));
            return ExitCode::SUCCESS;
        }
        Ok(Command::Version) => {
            println!("witdo-tui {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("{error}\n\n{}", usage("witdo-tui"));
            return ExitCode::from(2);
        }
    };
    let clock = Clock::system();
    let started = Profiles::from_system()
        .map_err(Into::into)
        .and_then(|profiles| start(&launch, profiles, &clock));
    let (profile, opening) = match started {
        Ok(started) => started,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let mut app = App::new(profile, opening, clock);
    match ratatui::run(|terminal| run(terminal, &mut app)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run(terminal: &mut DefaultTerminal, app: &mut App) -> io::Result<()> {
    loop {
        app.tick();
        terminal.draw(|frame| ui::draw(frame, app))?;
        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        if let Event::Key(key) = event::read()? {
            match app.handle_key(key) {
                Some(Action::Quit) => return Ok(()),
                Some(Action::OpenEditor(path)) => {
                    ratatui::restore();
                    let result = edit(&path);
                    *terminal = ratatui::init();
                    app.after_editor(result);
                }
                None => {}
            }
        }
    }
}

/// Runs `$VISUAL` or `$EDITOR` (falling back to `nano`) on a file and waits for it.
fn edit(path: &Path) -> io::Result<ExitStatus> {
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "nano".to_owned());
    let mut words = editor.split_whitespace();
    let program = words.next().unwrap_or("nano");
    Process::new(program).args(words).arg(path).status()
}
