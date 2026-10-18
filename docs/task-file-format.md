# Task file format

!!! note "Not built yet"
    This page describes the format witdo will read and write.

Every task is one Markdown file in the workspace's `tasks/` folder: YAML front
matter with its fields, then your notes.

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

## Fields

| Field          | Value                                     | Notes                                                        |
| -------------- | ----------------------------------------- | ------------------------------------------------------------ |
| `id`           | ULID, required                            | Never changes, whatever happens to the title or file name.   |
| `title`        | text, required                            | Cannot be empty.                                             |
| `status`       | required                                  | `inbox`, `todo`, `in_progress`, `done` or `cancelled`.       |
| `priority`     | required                                  | `low`, `normal`, `high` or `urgent`.                         |
| `created_at`   | date and time (UTC), required             |                                                              |
| `updated_at`   | date and time (UTC), required             | Updated on every change made in witdo.                       |
| `completed_at` | date and time (UTC), or `null`            | Set when the task is done or cancelled; cleared on reopening.|
| `due_date`     | `YYYY-MM-DD`, or `null`                   | A date in your local time zone.                              |
| `parent_id`    | task ID, or `null`                        | The task this one is a subtask of.                           |
| `depends_on`   | list of task IDs                          | Tasks that must be done before this one.                     |
| `category`     | text, or `null`                           | At most one per task.                                        |
| `tags`         | list of text                              | Any number.                                                  |

Optional fields can be left out. A file missing a required field, or with a
value witdo does not understand, is listed under **Problems** and not loaded;
witdo does not change it.

## What witdo keeps, and what it rewrites

- **Your notes** (everything after the front matter) are kept exactly as you
  wrote them.
- **Fields witdo does not know** are kept, with their values, after its own.
- **Comments and custom formatting** inside the front matter are not kept:
  witdo rewrites the front matter in its own layout when it saves a task.
- **File names** are `<title>--<short id>.md`, for example
  `call-the-bank--7tn3wd0r.md`. witdo renames the file when the title changes.
  The name is only for you: any `.md` file in `tasks/` with valid front matter
  is a task. Renaming does break Obsidian `[[wikilinks]]` to the old name.
- **Tags** are matched ignoring case, so `Writing` and `writing` are the same
  tag. Categories are matched exactly.

## Editing files outside witdo

You can edit, copy or delete task files with any tool, even while witdo is
open; it notices and reloads them. If you change a task in witdo that was
changed on disk in the meantime, witdo stops and asks whether to reload the
file or overwrite it.
