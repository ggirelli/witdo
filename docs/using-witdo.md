# Using witdo

Both apps show the same things and read and write the same files; pick
whichever suits the moment.

## Capture, then triage

New tasks go to the **Inbox**: type a title and press Enter. Later, open each
one and give it a status (**To do** or **In progress**), and whatever else
helps: a priority, a due date, a category, tags, notes.

## Views

| View                | Shows                                                       |
| ------------------- | ----------------------------------------------------------- |
| **Inbox**           | Tasks not triaged yet.                                      |
| **Next**            | Open, triaged tasks that nothing is blocking: what to do now. |
| **Today / overdue** | Open tasks due today or earlier.                            |
| **Open**            | Every open task.                                            |
| **Completed**       | Done and cancelled tasks, most recent first.                |
| **All**             | Everything.                                                 |
| **Categories**      | Tasks in each category, and those with none.                |

Lists show the soonest due first, then the most urgent, then the oldest. Switch
between a **list** and a **tree**, where subtasks sit under their parents; in a
tree, a task's parents (and in a category, its subtasks from other categories)
are shown dimmed, for context.

**Search** looks for text in titles and notes, in whatever view is open.
**Filters** narrow it further, by status, priority, due date (overdue, today,
this week, or none), category, tag, and parent (top level only, or everything
under a task): the **Filters** button in the desktop app, `f` in the terminal
app. Search and filters all apply together.

## Subtasks and dependencies

- A task can have one **parent**; set it in the task's details. Its category
  is its own: subtasks don't inherit one.
- A task can be **blocked by** other tasks, its prerequisites. While any of
  them is open, the task is *blocked* and stays out of **Next**. A cancelled
  prerequisite doesn't block, but is flagged so you can check it.
- witdo refuses links that make a loop, such as a task being its own
  grandparent.

## Done, reopened, deleted

- **Mark done** sets the task done. If it still has open prerequisites or
  subtasks, witdo says so first; it never changes other tasks for you.
- **Reopen** puts a done or cancelled task back to *To do*.
- **Delete** moves the task's file to `.witdo/trash/`. If it has subtasks,
  choose whether they move to the top level or go too; tasks that waited for
  it are told it is gone.

## Problems

Files can be edited elsewhere, so some may not make sense to witdo. The
**Problems** panel lists them in plain words: files that are not valid tasks,
copies with the same ID (none of them shows until one is left), links to tasks
that don't exist, and loops. witdo never fixes or deletes such files itself:
fix them in an editor, and witdo reloads them.

## When a file changes elsewhere

witdo notices when a task file changes, and shows the new version. If you
change a task in witdo that was changed elsewhere since you looked at it,
witdo stops and asks: **reload** (your change is dropped) or **overwrite**.

## Keys

### Desktop app

| Key          | Does                                     |
| ------------ | ---------------------------------------- |
| `n`          | Add a task (focus the capture field)     |
| `/`          | Search                                   |
| `↑` `↓`      | Move through the list                    |
| `Enter`      | Edit the selected task                   |
| `Esc`        | Close, clear the search, or leave a field |
| `Ctrl+Enter` | Mark done or reopen                      |
| `Delete`     | Delete the selected task                 |
| `Ctrl` `+` / `Ctrl` `-` | Zoom in / out (remembered per profile) |
| `Ctrl` `0`   | Back to 100%                             |

### Terminal app

| Key                   | Does                                          |
| --------------------- | --------------------------------------------- |
| `n`                   | Add tasks (Esc when done)                     |
| `/`                   | Search as you type                            |
| `f`                   | Filters (`←` `→` change one, `c` clears)      |
| `↑` `↓` or `j` `k`    | Move                                          |
| `Tab`                 | Next panel: views, list, task                 |
| `Enter`               | Open the task, or edit the chosen field       |
| `←` `→`               | Change status or priority, in the task panel  |
| `x`, `Space`          | Mark done or reopen (also `Ctrl+Enter`)       |
| `d`, `Delete`         | Delete                                        |
| `e`                   | Edit the task's file (and notes) in `$EDITOR` |
| `t`                   | List or tree                                  |
| `r`                   | Reload from disk                              |
| `w`                   | Switch workspace                              |
| `?`                   | All keys                                      |
| `q`                   | Quit                                          |

Due dates can be typed as `YYYY-MM-DD`, `today` or `tomorrow`.

`e` opens the task's Markdown file in `$VISUAL`, or else `$EDITOR`, or else
`nano`; witdo reloads the file when the editor closes.
