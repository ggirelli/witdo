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

## Front matter in detail

Amends §5. Decided 2026-10-18.

- **YAML crate:** `yaml-rust2`. Both serde forks the plan suggested
  (`serde_yaml_ng`, `serde_norway`) have not been updated since 2024;
  yaml-rust2 is maintained, keeps mappings in order (so unknown keys keep their
  order), reports line numbers, and can emit YAML for writing unknown keys back.
- **Known keys** are written as in the §5 example: double-quoted strings,
  `null`, and flow lists (`["writing", "q4"]`). **Unknown keys** are written in
  yaml-rust2's block style, so `aliases: [a]` comes back as `aliases:\n  - a`;
  the values are the same.
- **Reading is lenient** about YAML types: unquoted values, block lists, and
  numbers or booleans where text is expected (`title: 2026`) are all fine.
  Timestamps may have any offset and are read as UTC; witdo writes them in UTC
  with whole seconds.
- **The notes** start after the closing `---` line. The line break right after
  it (usually a blank line) is kept as it was, so the notes come back byte for
  byte; a new task's notes start after a blank line.
- A file may start with a UTF-8 byte order mark; witdo ignores it and does not
  write it back.
