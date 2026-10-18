# Amendments to plan v1

Decisions made while implementing [plan.v1.md](plan.v1.md), newest last. Each
section says which part of the plan it changes.

## `witdo-store` is now `witdo-io`

Amends §10. Decided 2026-10-18.

The crate that owns all workspace file I/O is called `witdo-io`. Its job is
unchanged: Markdown parse/serialize, workspace layout, scan, safe writes, trash
and watching, and it stays the only code that touches the filesystem (§9).

## A terminal UI next to the desktop one

Amends §10 and §11. Decided 2026-10-18.

witdo ships two front ends over the same `witdo-app`:

- `witdo-desktop`: the Dioxus desktop app, binary `witdo`.
- `witdo-tui`: a terminal app, binary `witdo-tui`, built with `ratatui`
  (crossterm backend).

It is a separate crate and binary, so it builds and runs without WebKitGTK and
works over SSH. Like the desktop UI, it calls `witdo-app` only and never
touches files; this keeps a second front end cheap and keeps all workspace I/O
behind `witdo-io` (§9).

Open questions, to settle before the UI phase (plan.v1.md §13, step 5):

- Whether the TUI reaches feature parity with the desktop app in the MVP, or
  starts with a subset (capture, views, complete/reopen) and grows.
- How it edits notes: an inline plain-text area, or handing off to `$EDITOR`.
- Whether `--profile` and `--workspace` handling is shared code in `witdo-app`
  (likely).

## The example task ID

Amends §5. Decided 2026-10-18.

The example `id` in plan.v1.md §5, `01JA4XKZ7Q3M8N2P5R6T4XK9Q2MB`, has 28
characters, so it is not a ULID (26). The docs and tests use
`01JA4XKZ7Q3M8N2P5R4XK9Q2MB` instead, which keeps the `4xk9q2mb` short ID of
the example file names.

## Views, ordering and filters in detail

Amends §6. Decided 2026-10-18.

- **All** is a view next to those in §6: every task, open or closed, so a
  search can reach completed tasks without switching views.
- **Order.** Completed lists the most recently closed first. Every other view,
  and siblings in the hierarchy, list the soonest due first (tasks without a
  date last), then the most urgent, then the oldest.
- **Due filter.** *Overdue* means open and due before today; a closed task is
  never overdue. *This week* is Monday to Sunday of the current week. A range
  includes both ends, and either end can be left open.
- **Within a filter part, any value matches** (e.g. status `todo` or
  `in_progress`); parts combine with AND. Tags are the exception: every listed
  tag must be present.
- **Subtree filter** includes the chosen task itself.
- **Missing prerequisites** (a `depends_on` ID with no task) do not block: the
  task is not waiting for anything witdo can see, and the Problems panel
  already lists the dangling reference.
- **Edits that keep existing problems are allowed.** The check on write only
  looks at links the edit adds: a new parent, or prerequisites not there
  before.
