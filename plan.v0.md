# WITDO --- Rust Task Manager

## Implementation brief for Claude Code

### 1. Product direction

**Working name:** WITDO (W-I-T-D-O)\
**Concept:** A task manager that helps a person understand what was,
what is, what comes next, and what to do.

WITDO should be a calm, private, local-first task manager---not a hosted
productivity service. Markdown files are the durable source of truth,
with an Obsidian-like feel and the option to inspect or edit tasks
outside the app.

### 2. Goals for the MVP

Build a small but useful desktop application that lets one person:

-   Capture tasks quickly in an inbox.
-   Create, edit, complete, reopen, and delete tasks.
-   Organize tasks into parent tasks and subtasks.
-   Represent explicit task dependencies, such as "A must happen before
    B."
-   Set status, priority, due date, category, tags, and notes.
-   Browse tasks as a flat list or hierarchical tree.
-   Search and filter tasks.
-   Store each task as a separate Markdown file with YAML front matter.
-   Work offline, with no account, server, telemetry, or mandatory
    network access.

### 3. Non-goals for the MVP

Do not implement these yet:

-   Cloud sync or collaboration.
-   Accounts, login, or a hosted backend.
-   Calendar integration.
-   Kanban/board view.
-   Recurring tasks or notifications.
-   AI features or automatic task generation.
-   Complex project-management features such as resource planning.
-   Mobile clients.

Keep the architecture open to future extensions, but do not build
speculative abstractions before they are needed.

### 4. User data and installations

The user wants separate local installations for personal tasks and work
tasks. Treat each installation/workspace as independent: its tasks,
settings, and index must not be mixed with another workspace.

At first launch, let the user choose or create a workspace directory.
Store task Markdown files inside that directory. Do not silently move,
upload, or merge user data.

Suggested directory layout:

``` text
WITDO/
  tasks/
    <task-id>.md
  .witdo/
    index.sqlite
    settings.toml
```

The Markdown files are authoritative. The SQLite index is a disposable
cache that can be rebuilt from the Markdown files at any time. If the
index is missing or corrupt, the application should be able to rebuild
it without losing task data.

Keep application-specific metadata in `.witdo/`, not in user-authored
task content unless necessary.

### 5. Suggested task file format

Use one Markdown file per task. Start with a simple, documented schema
and validate it when reading files.

``` markdown
---
id: "01JEXAMPLESTABLEID"
title: "Prepare project proposal"
status: "todo"
priority: "normal"
created_at: "2026-10-18T10:00:00Z"
updated_at: "2026-10-18T10:00:00Z"
due_date: null
parent_id: null
depends_on: []
categories: []
tags: []
---

## Notes

Additional task details go here.
```

Requirements:

-   Every task has a stable, unique ID that does not depend on its
    filename or title.
-   Use ISO 8601 timestamps.
-   Use a documented set of status and priority values.
-   `parent_id` identifies the parent task, if any.
-   `depends_on` contains task IDs that must be completed before this
    task can proceed.
-   Categories and tags are plain strings.
-   Preserve the Markdown body and unknown front-matter fields when
    practical, so future versions do not destroy data they do not
    understand.
-   Renaming a task should not change its ID.
-   Avoid relying on filenames as references between tasks.

Suggested initial values: - Status: `inbox`, `todo`, `in_progress`,
`done`, `cancelled` - Priority: `low`, `normal`, `high`, `urgent`

Use a date-only value for `due_date` unless the product has a clear
requirement for due times.

### 6. Core behaviour

#### Task lifecycle

-   Create a task from a title with minimal friction.
-   Edit title, notes, status, priority, due date, categories, tags,
    parent, and dependencies.
-   Mark a task done and reopen it.
-   Delete tasks with a clear confirmation where deletion could cause
    data loss.
-   Update `updated_at` when a task changes.

#### Parent tasks and subtasks

-   A task can have zero or one parent.
-   Display subtasks nested under their parent in hierarchy view.
-   Prevent parent-child cycles.
-   Deleting a parent must not silently delete its subtasks. Define a
    safe user-visible behaviour, such as asking whether to keep or
    delete children.

#### Dependencies

-   A task can depend on zero or more other tasks.
-   Prevent self-dependencies and dependency cycles.
-   Show which tasks block a task and which tasks it blocks.
-   In the MVP, warn when a user completes a task that still has
    incomplete prerequisites; do not silently change other task
    statuses.
-   Keep dependency relationships based on stable task IDs.

#### Search and filtering

Support search over task titles and Markdown notes, plus filters for: -
Status - Priority - Due date - Category - Tag - Parent / hierarchy

Provide useful default views: - Inbox - All open tasks - Today / overdue
tasks (based on due dates) - Completed tasks

Keep the view logic independent from the storage implementation.

### 7. Technical architecture

Use Rust for the application and keep the domain logic independent from
the UI framework and persistence details.

Suggested crates/modules:

``` text
witdo/
  crates/
    witdo-core/       # Task model, validation, domain rules, dependency graph
    witdo-storage/    # Markdown parsing/writing, workspace scanning, SQLite index
    witdo-app/        # Application services and use cases
    witdo-desktop/    # Dioxus desktop UI and application entry point
```

This is a suggested starting point, not a reason to over-engineer. If a
workspace with fewer crates is more appropriate initially, keep clear
module boundaries and extract crates when useful.

Principles: - Core domain types and rules must not depend on Dioxus. -
The storage layer owns Markdown serialization, file operations, and
index rebuilding. - The UI calls application services rather than
manipulating files directly. - Keep file I/O and potentially slow
indexing off the UI thread. - Return actionable errors; do not swallow
parse or write failures. - Write task updates safely, using a temporary
file and atomic replacement where supported. - Consider external edits
to Markdown files. Detect changes and refresh the index; do not
overwrite a user's external edits without handling conflicts. - Keep the
application usable offline. - Add tests for domain rules and storage
behaviour before adding UI complexity.

### 8. SQLite index

SQLite is an implementation detail, not the canonical data store.

Use it only to make listing, filtering, and searching efficient if
needed. The app must be able to: 1. Scan task Markdown files. 2. Parse
and validate their front matter. 3. Rebuild the index. 4. Report
malformed files without silently deleting or rewriting them.

Keep the initial indexing strategy simple. Do not introduce a database
migration framework unless the implementation genuinely needs one.

### 9. UI direction

Build a functional desktop UI with Dioxus. Prioritize clarity and speed
over visual polish.

Initial layout: - A navigation area for Inbox, Open Tasks,
Today/Overdue, Completed, and categories. - A central task list or
hierarchy. - A task detail/editor panel. - A clear action to add a task
from anywhere.

The user should be able to create a task quickly and understand its
status, priority, due date, parent, and dependency state without opening
multiple dialogs. Make keyboard navigation and basic shortcuts possible,
but avoid delaying core functionality for a comprehensive shortcut
system.

### 10. Testing and acceptance criteria

The MVP is ready when all of the following work:

1.  A user can create a workspace in a chosen local directory.
2.  Creating a task creates a readable Markdown file with valid front
    matter.
3.  Restarting the app loads the same tasks from Markdown.
4.  Deleting the SQLite index and restarting rebuilds it from Markdown
    without losing tasks.
5.  Editing a task preserves its stable ID and Markdown notes.
6.  Parent/subtask relationships display correctly and cycles are
    rejected.
7.  Dependencies display correctly; self-dependencies and cycles are
    rejected.
8.  Search and the basic filters return expected results.
9.  Completing and reopening tasks persists across restarts.
10. Malformed task files produce understandable errors and are not
    silently discarded.
11. Personal and work workspaces remain independent.
12. Core domain and storage rules have automated tests.

### 11. Suggested implementation sequence

**Phase 1 --- Project skeleton** - Inspect the existing repository
before changing anything. - Establish the Rust workspace and buildable
Dioxus desktop shell. - Add formatting, linting, and test commands. -
Document how to run the application.

**Phase 2 --- Domain model** - Define task, status, priority, ID, and
validation types. - Implement parent-cycle and dependency-cycle
validation. - Add unit tests.

**Phase 3 --- Markdown storage** - Implement task serialization and
parsing. - Implement workspace scanning, create/update/delete, safe
writes, and index rebuild. - Add tests using temporary directories.

**Phase 4 --- Application services** - Add use cases for create, edit,
complete, reopen, list, search, filter, and dependency inspection. -
Keep these independent of the UI.

**Phase 5 --- Functional UI** - Implement workspace selection, task
creation/editing, inbox, list and hierarchy views, details, search, and
filters. - Connect the UI to application services.

**Phase 6 --- Reliability pass** - Test restart persistence,
corrupt/missing index recovery, malformed files, external edits, and
workspace separation. - Fix data-loss and usability issues before adding
features.

### 12. Working instructions for the coding agent

Before implementing: 1. Inspect the repository and summarize its current
structure and relevant conventions. 2. Identify decisions that are
genuinely blocked by missing information; otherwise use the defaults in
this brief. 3. Propose a short implementation plan and then execute it
in small, buildable steps. 4. Run formatting, linting where configured,
and tests after meaningful changes. 5. Keep the application compiling
throughout development. 6. Do not implement out-of-scope features just
because they may be useful later. 7. Update the README with setup, run,
test, and workspace/data-format instructions. 8. Report what changed,
the commands run, test results, and any known limitations.

### 13. Open decisions

Use sensible defaults unless implementation is blocked: - Exact Dioxus
version and desktop backend, based on the repository's current setup. -
File naming convention for task IDs. - Whether to include a lightweight
Markdown editor or a plain text notes editor initially. - How to surface
conflicting external edits.

Do not block the initial skeleton on branding, logos, themes, or
advanced configuration. WITDO is the working name; focus first on a
reliable local-first MVP.

### 14. Version control: Jujutsu (jj)

The repository must use **Jujutsu (`jj`)** for version control. Do not
initialize Git as the primary workflow.

Before making repository changes: 1. Inspect whether the current
directory is already a `jj` workspace/repository. 2. If it is not,
initialize a Jujutsu repository using the appropriate `jj` command for
the installed version. 3. Check the installed `jj` version and use
commands supported by that version. 4. Do not overwrite, discard, or
migrate existing user data or repository history without explicit
approval. 5. If the directory already contains Git history or is
colocated with Git, inspect the setup and explain the implications
before changing it. Do not assume that Git metadata can be safely
removed.

For ongoing work: - Use Jujutsu concepts and commands in the development
workflow. - Make coherent, small changes that are easy to inspect and
undo. - Before substantial changes, inspect the working-copy status and
existing changes; do not overwrite unrelated user work. - After each
meaningful implementation phase, summarize the changes and relevant `jj`
state. - Document the basic `jj` workflow in the README, including how
to inspect changes, create a change, describe it, and undo/revert
safely. - Do not require GitHub or another hosted Git service for local
development. If remote hosting is later requested, discuss the available
Jujutsu-compatible workflow before configuring it.

The coding agent should explain any Jujutsu-specific setup issue rather
than silently falling back to a Git-only workflow.
