//! witdo: what was, what is, what then, do.

mod model;
mod view;

use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use dioxus::desktop::{Config, LogicalSize, WindowBuilder};
use dioxus::prelude::*;
use witdo_app::{Clock, Command, Profiles, parse_args, start, usage};

use crate::model::Model;

const STYLE: &str = include_str!("style.css");

/// Hands the model built before launch to the root component, once.
#[derive(Clone)]
struct Startup(Arc<Mutex<Option<Model>>>);

fn main() -> ExitCode {
    let launch = match parse_args(
        std::env::args_os().skip(1),
        std::env::var_os("WITDO_PROFILE"),
    ) {
        Ok(Command::Run(launch)) => launch,
        Ok(Command::Help) => {
            print!("{}", usage("witdo"));
            return ExitCode::SUCCESS;
        }
        Ok(Command::Version) => {
            println!("witdo {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("{error}\n\n{}", usage("witdo"));
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
    let window = profile.config().window;
    let size = LogicalSize::new(
        f64::from(window.width.unwrap_or(1200)),
        f64::from(window.height.unwrap_or(780)),
    );
    let model = Model::new(profile, opening, clock);
    let window = WindowBuilder::new()
        .with_title("witdo")
        .with_inner_size(size);
    dioxus::LaunchBuilder::desktop()
        .with_cfg(
            Config::new()
                .with_window(window)
                .with_menu(None)
                .with_custom_head(format!("<style>{STYLE}</style>")),
        )
        .with_context(Startup(Arc::new(Mutex::new(Some(model)))))
        .launch(Root);
    ExitCode::SUCCESS
}

#[component]
fn Root() -> Element {
    let startup = use_context::<Startup>();
    use_context_provider(move || {
        let model = startup
            .0
            .lock()
            .ok()
            .and_then(|mut model| model.take())
            .expect("the model is handed over once, at launch");
        Signal::new(model)
    });
    rsx! { view::App {} }
}
