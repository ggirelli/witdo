# Development

witdo is written in Rust. The desktop app uses [Dioxus](https://dioxuslabs.com)
and the terminal app [ratatui](https://ratatui.rs); both sit on the same core.

You need Rust, [`just`](https://github.com/casey/just),
[`cargo-deny`](https://github.com/EmbarkStudios/cargo-deny) and, for these docs,
[`uv`](https://docs.astral.sh/uv/). See [getting started](getting-started.md)
for the Linux packages the desktop app needs.

```sh
just run          # the desktop app
just tui          # the terminal app
just check        # formatting, lints, tests and licence checks, as in CI
just docs         # this site, live at http://127.0.0.1:8000
just docs-build   # build this site into ./site, failing on warnings
```

## Crates

| Crate           | What it does                                                         |
| --------------- | -------------------------------------------------------------------- |
| `witdo-core`    | Tasks, validation, hierarchy and dependencies, views, search, filters |
| `witdo-io`      | Task files, workspace layout, scanning, safe writes, trash, watching  |
| `witdo-app`     | Use cases: the task lifecycle, workspaces and profiles               |
| `witdo-desktop` | The desktop app, binary `witdo`                                      |
| `witdo-tui`     | The terminal app, binary `witdo-tui`                                 |

`witdo-core` knows nothing about files or the UI, all workspace file I/O goes
through `witdo-io`, and the UIs call `witdo-app` only.

The design and its decisions are in
[`design/`](https://github.com/ggirelli/witdo/tree/main/design), and the coding
and commit style in
[CONTRIBUTING.md](https://github.com/ggirelli/witdo/blob/main/CONTRIBUTING.md).
