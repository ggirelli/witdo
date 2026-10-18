![witdo](assets/brand/witdo-banner.webp)

# witdo

**What was, what is, what then, do.**

witdo is a calm, private, local-first task manager, as a desktop app or in the
terminal. Your tasks are plain Markdown files in a folder you choose, pleasant
to read and edit in Obsidian or any text editor. There is no account, server,
telemetry or network access.

> **Status:** first version, not released yet. Both apps manage tasks:
> capture, views, subtasks, dependencies, search and filters, profiles and
> workspaces. Build it from source as below; see the
> [user guide](docs/using-witdo.md) for how to use it.

## Setting up

You need:

- Rust: the version pinned in [`rust-toolchain.toml`](rust-toolchain.toml) is
  installed automatically by `rustup`.
- [`just`](https://github.com/casey/just) to run the project's recipes.
- [`cargo-deny`](https://github.com/EmbarkStudios/cargo-deny) for the dependency
  checks.
- On Linux, for the desktop app only, the WebKitGTK and libsoup development
  packages, plus `xdo`, for the desktop window. On Debian or Ubuntu:

  ```sh
  sudo apt install libwebkit2gtk-4.1-dev libsoup-3.0-dev libxdo-dev build-essential pkg-config
  ```

  On Fedora:

  ```sh
  sudo dnf install webkit2gtk4.1-devel libsoup3-devel libxdo-devel
  ```

## Running and testing

```sh
just          # list the recipes
just run      # run the desktop app
just tui      # run the terminal app (q or Esc quits)
just build    # release build of both apps
just check    # everything CI checks: rustfmt, clippy (pedantic, warnings are errors), tests, cargo-deny
just docs     # the user guide, live at http://127.0.0.1:8000 (needs uv)
```

`just fmt`, `just clippy`, `just test` and `just deny` run the individual
checks.

The user guide lives in [`docs/`](docs/) and is built with MkDocs into
[ggirelli.github.io/witdo](https://ggirelli.github.io/witdo/).

## Profiles and workspaces

A **workspace** is a folder of tasks, entirely
self-contained. A **profile** is a separate set of app state (known workspaces,
window preferences), so a "work" profile never sees a "personal" one. Choose one
with `witdo --profile <name>` or `WITDO_PROFILE=<name>`, and open a folder
directly with `witdo --workspace <path>`.

## File format

One Markdown file per task in the workspace's `tasks/`
folder, with YAML front matter for its fields and your notes below. See [§5 of
the brief](design/plan.v1.md#5-task-file-format).

## Project layout

```text
crates/
  witdo-core/      task model, hierarchy and dependencies, views, search, filters
  witdo-io/        Markdown files, workspace layout, safe writes, trash, watching
  witdo-app/       use cases: task lifecycle, workspaces and profiles
  witdo-desktop/   Dioxus desktop UI and the `witdo` binary
  witdo-tui/       terminal UI and the `witdo-tui` binary
design/            plans and design notes
assets/brand/      banner and icon
```

## Version control

The repository uses [Jujutsu](https://jj-vcs.github.io/jj/), colocated with Git.
Day to day:

```sh
jj st                          # inspect working-copy changes
jj log                         # inspect history
jj describe -m "feat: …"       # describe the current change
jj new                         # start a new change on top
jj git push -c @-              # push the finished change to a new branch
gh pr create --fill            # open a pull request for it
jj git fetch                   # bring in main once it is merged
jj undo                        # undo the last jj operation
```

`main` is protected: changes land through pull requests, squashed or rebased
so history stays linear, once the checks pass. Commits must be signed.

See [CONTRIBUTING.md](CONTRIBUTING.md) for the coding and commit style.

## Licence

[MIT](LICENSE).
