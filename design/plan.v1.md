# witdo — Rust task manager

Implementation brief, version 1. Supersedes [plan.v0.md](plan.v0.md).

## 1. Product direction

**Name:** witdo\
**Concept:** a task manager that helps a person understand what was, what
is, what comes next, and what to do.

witdo is a calm, private, local-first desktop task manager. Plain Markdown
files are the only source of truth. They should be pleasant to read and
edit in other tools such as Obsidian or a text editor.

## 2. MVP goals

One person can:

- Capture tasks quickly into an inbox.
- Create, edit, complete, reopen, and delete tasks.
- Organize tasks into parent tasks and subtasks.
- Declare dependencies ("A must be done before B").
- Set status, priority, due date, category, tags, and notes.
- Browse tasks as a flat list or as a hierarchy.
- Search and filter tasks.
- Keep separate profiles and workspaces (e.g. personal and work) that never
  mix.
- Work fully offline: no account, server, telemetry, or network access.

## 3. Non-goals for the MVP

- Cloud sync, collaboration, accounts, hosted backend.
- Calendar integration, Kanban view, recurring tasks, notifications.
- AI features or automatic task generation.
- Resource planning or other heavy project-management features.
- Mobile clients.
- Any database or persistent index (see §8).
- Application-level encryption (designed for, not built; see §9).
- A rich Markdown editor; notes use a plain text editor.

Keep the architecture open to these, but do not build speculative
abstractions for them.

## 4. Profiles and workspaces

Separation currently works by running separate installations. witdo must
also support it within one installation, at two levels:

- **Workspace:** a directory containing tasks. It is fully self-contained:
  every task, setting, and piece of app metadata for that workspace lives
  inside it. Nothing about a workspace is stored anywhere else, except its
  path in a profile's recent list.
- **Profile:** an isolated set of global app state: the list of known
  workspaces, the last opened workspace, and window preferences. A profile
  can know about several workspaces. Profiles don't share anything, so a
  "work" profile never reveals the paths of a "personal" profile.

Profile selection:

- `witdo --profile <name>` or `WITDO_PROFILE=<name>`; default profile is
  `default`.
- Profile state lives at `$XDG_CONFIG_HOME/witdo/profiles/<name>/config.toml`
  (platform-equivalent directories on other OSes, via the `directories`
  crate).
- `witdo --workspace <path>` opens a workspace directly and adds it to the
  current profile's recent list.

Behaviour:

- One window shows exactly one workspace. Two windows/processes may be open
  at once, on different workspaces or profiles.
- On first launch for a profile, the user picks or creates a workspace
  directory.
- A directory is a workspace if it contains `.witdo/settings.toml`.
  Initializing a non-empty directory requires confirmation; existing files
  are never moved, renamed, or modified.
- witdo never silently moves, uploads, or merges data between workspaces.

### Workspace layout

```text
my-workspace/
  tasks/
    prepare-project-proposal--4xk9q2mb.md
    call-the-bank--7tn3wd0r.md
  .witdo/
    settings.toml
    trash/
    tmp/
```

- `tasks/` is flat. Subdirectories are ignored and reported as a warning.
- `.witdo/settings.toml` holds `schema_version = 1` and an optional
  workspace display `name`. Keep it minimal.
- `.witdo/trash/` receives deleted task files (§6).
- `.witdo/tmp/` holds files being written atomically; being inside the
  workspace guarantees the same filesystem as `tasks/`, so the final rename
  is atomic.

## 5. Task file format

One Markdown file per task: YAML front matter plus a free-form body.

```markdown
---
id: "01JA4XKZ7Q3M8N2P5R6T4XK9Q2MB"
title: "Prepare project proposal"
status: "todo"
priority: "normal"
created_at: "2026-10-18T10:00:00Z"
updated_at: "2026-10-18T10:00:00Z"
completed_at: null
due_date: "2026-10-25"
parent_id: null
depends_on: []
category: "Work"
tags: ["writing", "q4"]
---

Draft the outline first. Ask Maria for last year's numbers.
```

### Fields

| Field          | Type                         | Notes                                                       |
| -------------- | ---------------------------- | ----------------------------------------------------------- |
| `id`           | ULID string, required        | Stable forever; independent of title and filename.          |
| `title`        | non-empty string, required   |                                                             |
| `status`       | enum, required               | `inbox`, `todo`, `in_progress`, `done`, `cancelled`.        |
| `priority`     | enum, required               | `low`, `normal`, `high`, `urgent`.                          |
| `created_at`   | RFC 3339 UTC, required       |                                                             |
| `updated_at`   | RFC 3339 UTC, required       | Updated on every change made through witdo.                 |
| `completed_at` | RFC 3339 UTC or null         | Set when status becomes `done`/`cancelled`; cleared on reopen. |
| `due_date`     | `YYYY-MM-DD` or null         | Date only; interpreted in the user's local timezone.        |
| `parent_id`    | task ID or null              | Zero or one parent.                                         |
| `depends_on`   | list of task IDs             | Prerequisites of this task.                                 |
| `category`     | string or null               | Optional, at most one per task. Flat; `/` has no special meaning. |
| `tags`         | list of strings              | Any number; independent of category.                        |

Missing optional fields are treated as their empty value. Missing required
fields make the file invalid (§7).

### Rules

- **IDs:** ULIDs (26 characters, Crockford base32), generated at creation.
  Every reference between tasks uses IDs, never filenames.
- **Filenames:** `<slug>--<short-id>.md`, where `slug` is the title
  lowercased, ASCII-transliterated, non-alphanumerics collapsed to `-`,
  truncated to 60 characters, and `short-id` is the last 8 characters of the
  ULID, lowercased (extended if it would collide). witdo renames the file
  when the title changes. Filenames are cosmetic: any `*.md` file in
  `tasks/` with valid front matter is a task, whatever its name. Known
  limitation: renaming breaks Obsidian wikilinks that point at the old
  filename.
- **Body:** everything after the front matter is the task's notes,
  preserved byte-for-byte. New tasks start with an empty body; witdo does
  not generate headings.
- **Unknown front-matter keys** are preserved, along with their values, and
  written back after the known keys in their original order. YAML comments
  and custom formatting in the front matter are not preserved; document
  this.
- **Known keys** are written in the canonical order shown above.
- **Tags** are stored as typed (trimmed, no leading `#`), compared
  case-insensitively, and deduplicated case-insensitively.
- **Category** is compared case-sensitively and shown as typed.

## 6. Core behaviour

### Lifecycle

- **Capture:** a single title field, available from anywhere, creates a task
  with status `inbox` and priority `normal`.
- **Edit:** title, notes, status, priority, due date, category, tags, parent,
  and dependencies.
- **Open vs closed:** `inbox`, `todo`, and `in_progress` are open; `done`
  and `cancelled` are closed.
- **Complete:** sets `done` and `completed_at`. Warn (do not block) when the
  task has open prerequisites or open subtasks. Never change other tasks'
  statuses automatically.
- **Reopen:** sets status `todo` and clears `completed_at`.
- **Delete:** moves the file to `.witdo/trash/` (with a timestamp suffix);
  never deletes permanently. Requires confirmation that states the
  consequences:
  - If the task has subtasks, ask whether to move the subtasks to the top
    level (default) or delete them too.
  - If other tasks depend on it, list them; on confirmation, remove the ID
    from their `depends_on` (these edits are visible and update their
    `updated_at`).

### Hierarchy

- Zero or one parent per task; parent-child cycles are rejected on write.
- Subtasks are not bound to their parent's category: each task's category
  is independent (including none), nothing is inherited, and changing a
  parent's category never touches its subtasks.
- Hierarchy view nests subtasks under their parent regardless of category.
  A category view shows the tasks in that category; their parents or
  subtasks in other categories appear as context (dimmed), not as members.

### Dependencies

- Zero or more prerequisites per task, in any category or none.
  Self-dependencies and cycles are rejected on write.
- A task is **blocked** while any prerequisite is open. A `cancelled`
  prerequisite does not block, but the UI marks it ("prerequisite
  cancelled") so the user can review.
- The task detail shows both directions: "blocked by" and "blocks".

### Views

- **Inbox:** status `inbox`.
- **Next:** open, not `inbox`, not blocked. This is the "what to do now"
  view.
- **Today / overdue:** open tasks with `due_date` ≤ today (local date).
- **Open:** all open tasks.
- **Completed:** closed tasks, newest `completed_at` first.
- **Category:** one entry per category in use, plus **Uncategorized**.

### Search and filtering

- Text search: case-insensitive substring match over title and body.
- Filters: status, priority, due date (overdue / today / this week / none /
  range), category, tag, and parent (top-level only / subtree of a task).
- Filters combine with AND and apply to any view.
- In hierarchy view, matching tasks are shown with their ancestors (dimmed
  if they don't match) so the tree stays readable.
- View and filter logic lives in the domain layer, operating on in-memory
  tasks; it knows nothing about files.

## 7. Integrity and external edits

Files may be edited, copied, or deleted outside witdo at any time. witdo
loads everything it can and reports the rest; it **never** rewrites,
deletes, or "fixes" a file on its own.

A **Problems** panel lists:

- **Malformed files:** unparseable YAML, missing required fields, invalid
  enum values or dates. Shown with file path, line (when known), and a
  plain-language message. The task is not loaded.
- **Duplicate IDs:** two files with the same `id` (typically a copied file).
  Both are listed; neither can be edited until the user resolves it.
- **Dangling references:** `parent_id` or `depends_on` pointing at an
  unknown ID. The task loads; the reference is shown as missing.
- **Cycles loaded from disk:** the task loads; tasks in a parent cycle are
  shown at the top level of the hierarchy with a warning; dependency cycles
  are flagged on each task involved.
- **Ignored entries:** subdirectories or non-`.md` files in `tasks/`.

Editing a task that has problems is allowed as long as the edit doesn't
introduce new ones.

### Change detection and conflicts

- witdo watches `tasks/` (the `notify` crate, debounced) and re-parses
  changed files.
- For each loaded file, witdo remembers a hash of its contents. Before
  writing, it re-reads the file: if the hash differs, the write is aborted
  and the user chooses **Reload** (discard my changes) or **Overwrite**
  (explicit confirmation).
- Changes witdo itself writes are recognized by hash and don't trigger
  reloads.

### Safe writes

Write to `.witdo/tmp/`, fsync, then rename over the target. A write failure
is reported to the user and leaves the original file untouched.

## 8. No database

There is no SQLite or other persistent index. On opening a workspace, witdo
scans `tasks/`, parses every file, and builds in-memory structures (task
map, child lists, reverse dependency lists). Search and filters run over
that. This is fast enough for thousands of tasks.

If startup ever becomes slow, a cache may be added later strictly as an
optimization — a disposable file whose deletion never matters. Not in
the MVP.

## 9. Encryption readiness

Fully encrypted workspaces are a likely future feature, not part of the
MVP. Today, users who need encryption at rest can put a workspace inside an
encrypted volume (LUKS, gocryptfs, Cryptomator, VeraCrypt); witdo must work
correctly there.

To keep application-level encryption possible later without a rewrite:

- All workspace file I/O goes through one narrow storage interface in the
  storage crate (read task, write task, list tasks, move to trash, watch).
  No other code touches the filesystem directly.
- No task content is ever written outside the workspace: no caches, logs,
  crash dumps, or temp files elsewhere. Profile config contains only paths
  and preferences.
- Logs never include titles or notes.
- Trash and temp files stay inside the workspace, so they would be
  encrypted along with everything else.

Known trade-off to record when encryption is designed: encrypted task files
are no longer readable in Obsidian or a text editor.

## 10. Technical architecture

Rust (edition 2024) workspace:

```text
witdo/
  crates/
    witdo-core/      # Task model, validation, hierarchy/dependency graph, views, search, filters
    witdo-store/     # Markdown parse/serialize, workspace layout, scan, safe writes, trash, watcher
    witdo-app/       # Use cases: create/edit/complete/reopen/delete, workspace & profile handling
    witdo-desktop/   # Dioxus desktop UI and binary entry point
  design/            # Plans and design notes
```

Principles:

- `witdo-core` depends on neither Dioxus nor the filesystem.
- `witdo-store` owns all file I/O (§9).
- The UI calls `witdo-app` only; it never touches files.
- File I/O, scanning, and watching run off the UI thread.
- Errors are actionable and never swallowed.
- Tests for domain and storage rules come before UI complexity.

Library choices (confirm current maintenance status when adding them):

- `ulid` for IDs; `serde` for (de)serialization.
- A maintained serde YAML crate (e.g. `serde_yaml_ng` or `serde_norway`;
  `serde_yaml` is deprecated); `indexmap` for preserving unknown keys in
  order.
- `jiff` (or `chrono`) for timestamps and local dates.
- `notify` + `notify-debouncer-full` for file watching.
- `directories` for platform config paths.
- `thiserror` for error types; `tempfile` in tests.
- Dioxus 0.7 desktop. On Linux this needs the webkit2gtk-4.1 and libsoup-3
  development packages; document them in the README.

Tooling: `cargo fmt`, `cargo clippy --all-targets -- -D warnings`,
`cargo test --workspace`. Pin the toolchain with `rust-toolchain.toml`.

## 11. UI direction

Functional, clear, fast; polish later.

- **Left:** profile/workspace indicator and switcher; views (Inbox, Next,
  Today/Overdue, Open, Completed); categories; Problems (with count).
- **Center:** task list or hierarchy, toggleable. Each row shows status,
  title, priority, due date, category, and a blocked indicator.
- **Right:** detail editor: all fields, plain-text notes, "blocked by" and
  "blocks" lists, parent and subtasks.
- **Always available:** capture field.

Minimal shortcuts: `n` capture, `/` search, arrow keys to move through the
list, `Enter` to open, `Esc` to close or cancel, `Ctrl+Enter` to toggle
done. No configurable shortcut system.

## 12. Acceptance criteria

1. A user can create a workspace in a chosen local directory, and open it
   again later from the profile's recent list.
2. Creating a task creates a readable `<slug>--<short-id>.md` file with
   valid front matter.
3. Restarting the app loads the same tasks from Markdown.
4. The app keeps no task state outside the Markdown files; deleting
   `.witdo/tmp/` or the profile config loses no tasks.
5. Editing a task preserves its ID, its body byte-for-byte, and unknown
   front-matter keys. Changing the title renames the file.
6. Parent/subtask relationships display correctly; cycles are rejected.
7. Dependencies display in both directions; self-dependencies and cycles
   are rejected; blocked tasks are excluded from Next.
8. Search and filters return expected results.
9. Completing and reopening tasks persists across restarts.
10. Malformed files, duplicate IDs, dangling references, and on-disk cycles
    appear in Problems with understandable messages; no file is modified or
    discarded.
11. An external edit to a task file shows up in the app; a save that would
    overwrite an external edit is stopped and offers Reload/Overwrite.
12. Deleting a task moves it to `.witdo/trash/`, handles subtasks and
    dependents as described in §6.
13. Two profiles, and two workspaces in one profile, stay independent.
14. Core domain and storage rules have automated tests.

## 13. Implementation sequence

Each phase ends compiling, formatted, lint-clean, and tested, as its own jj
change.

1. **Skeleton:** Cargo workspace with the four crates, `rust-toolchain.toml`,
   a Dioxus desktop window that opens, fmt/clippy/test commands, README.
2. **Domain:** task types, enums, validation, hierarchy and dependency
   graph with cycle detection, views, search, filters. Unit tests.
3. **Storage:** front-matter parse/serialize with round-trip tests
   (body and unknown keys), slugs and filenames, scan with problem
   reporting, safe writes, rename on title change, trash, conflict
   detection by hash. Tests on temporary directories.
4. **Application services:** use cases from §6, profile and workspace
   management, watcher integration. Tests with temporary workspaces.
5. **UI:** profile/workspace selection, capture, list and hierarchy views,
   detail editor, search, filters, Problems panel, shortcuts.
6. **Reliability pass:** walk through §12 end to end; fix data-loss and
   usability issues before adding anything.

## 14. Working instructions for the coding agent

1. Inspect the repository state before changing anything.
2. Use this brief's decisions; ask only when genuinely blocked.
3. Work in small, buildable steps; keep everything compiling.
4. Run fmt, clippy, and tests after meaningful changes.
5. Do not implement non-goals.
6. Keep the README current: setup, system dependencies, run, test,
   profile/workspace usage, file format, and the jj workflow.
7. After each phase, report what changed, commands run, test results,
   known limitations, and jj state.
8. New design decisions go into a new `design/plan.vN.md` (or a focused
   note in `design/`), not into code comments alone.

## 15. Version control (Jujutsu)

The repository is set up:

- Jujutsu repository, colocated with Git (`.jj/` and `.git/` side by side).
- Remote `origin`: `https://github.com/ggirelli/witdo.git`; bookmark `main`
  tracks `main@origin`.

Workflow:

```sh
jj st                          # inspect working-copy changes
jj log                         # inspect history
jj describe -m "message"       # describe the current change
jj new                         # start a new change on top
jj bookmark set main -r @-     # move main to the finished change
jj git push                    # publish main
jj undo                        # undo the last jj operation
jj op log                      # inspect operation history
```

- Use jj commands, not Git commands, for day-to-day work.
- One coherent change per logical step; describe it clearly.
- Check `jj st` before substantial work; never overwrite unrelated changes.
- Push to `origin` only once a phase is finished and passes checks.
- Explain jj-specific problems rather than falling back to Git.

## 16. Decisions made since v0

| Topic                 | Decision                                                             |
| --------------------- | -------------------------------------------------------------------- |
| Name                  | witdo                                                                |
| IDs                   | ULID                                                                 |
| Filenames             | `<slug>--<short-id>.md`, renamed with title; flat `tasks/`           |
| SQLite                | Dropped; in-memory scan                                              |
| Separation            | Profiles (global state) × workspaces (self-contained dirs)           |
| Encryption            | Not in MVP; storage boundary and no-leak rules keep it possible      |
| Category              | Optional, at most one per task; not inherited by subtasks            |
| Tags                  | Free-form, many per task, cross categories                           |
| Cross-category links  | Parent/subtask and dependency links may span categories              |
| Cancelled prerequisite| Does not block; flagged for review                                   |
| Reopen                | Back to `todo`                                                       |
| Delete                | Move to `.witdo/trash/`; ask about subtasks; unlink dependents       |
| Body                  | Preserved verbatim; no generated headings                            |
| Conflicts             | Hash check before write; Reload or Overwrite                         |
| Notes editor          | Plain text                                                           |
| Next view             | Added: open, triaged, unblocked                                      |
