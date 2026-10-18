# Crate names and the terminal UI

Amends [plan.v1.md §10](plan.v1.md#10-technical-architecture). Decided 2026-10-18.

## `witdo-store` is now `witdo-io`

The crate that owns all workspace file I/O is called `witdo-io`. Its job is unchanged: Markdown
parse/serialize, workspace layout, scan, safe writes, trash and watching, and it stays the only
code that touches the filesystem (§9).

## A terminal UI next to the desktop one

witdo ships two front ends over the same `witdo-app`:

- `witdo-desktop`: the Dioxus desktop app, binary `witdo`.
- `witdo-tui`: a terminal app, binary `witdo-tui`, built with `ratatui` (crossterm backend).

It is a separate crate and binary, so it builds and runs without WebKitGTK and works over SSH.
Like the desktop UI, it calls `witdo-app` only and never touches files; this keeps a second
front end cheap and keeps all workspace I/O behind `witdo-io` (§9).

Open questions, to settle before the UI phase (plan.v1.md §13, step 5):

- Whether the TUI reaches feature parity with the desktop app in the MVP, or starts with a
  subset (capture, views, complete/reopen) and grows.
- How it edits notes: an inline plain-text area, or handing off to `$EDITOR`.
- Whether `--profile` and `--workspace` handling is shared code in `witdo-app` (likely).
