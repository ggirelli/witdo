//! witdo: what was, what is, what comes next, and what to do.

use dioxus::desktop::{Config, WindowBuilder};
use dioxus::prelude::*;

fn main() {
    let window = WindowBuilder::new().with_title("witdo");
    dioxus::LaunchBuilder::desktop()
        .with_cfg(Config::new().with_window(window))
        .launch(App);
}

#[component]
fn App() -> Element {
    rsx! {
        main {
            h1 { "witdo" }
            p { "What was, what is, what comes next, and what to do." }
        }
    }
}
