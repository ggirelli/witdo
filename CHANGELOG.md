# Changelog

All notable changes to witdo are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Tasks as Markdown files with YAML front matter, one per task, in a
  workspace's `tasks/` folder. witdo keeps your notes byte for byte, and
  fields it does not know, when it saves a task.
- A desktop app (`witdo`) and a terminal app (`witdo-tui`) over the same files.
- Capture into an inbox; views for Next, Today / overdue, Open, Completed, All
  and each category; a list or a tree of subtasks; search.
- Subtasks and dependencies: a task waiting for open prerequisites is blocked
  and stays out of Next; loops are refused.
- Mark done (with a warning about open prerequisites and subtasks), reopen, and
  delete into `.witdo/trash/`.
- A Problems panel for files that do not load, copies with the same ID, links
  to missing tasks and loops; witdo never changes such files itself.
- Changes made to task files elsewhere are picked up while witdo is open, and a
  save that would overwrite them asks first.
- Profiles (`--profile`, `WITDO_PROFILE`) and workspaces (`--workspace`) that
  keep personal and work tasks apart.
- A user guide in `docs/`, built with MkDocs (`just docs`).
