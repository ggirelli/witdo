# Profiles and workspaces

## Workspaces

A **workspace** is a folder of tasks. It is entirely self-contained: every
task, setting and piece of witdo's own data for that workspace lives inside
it, so you can copy, back up or sync the folder and lose nothing.

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

- `tasks/` holds one Markdown file per task (see the
  [task file format](task-file-format.md)).
- `.witdo/settings.toml` marks the folder as a workspace.
- `.witdo/trash/` receives deleted tasks: witdo never deletes a file for good.
- `.witdo/tmp/` holds files while they are being saved.

You can turn a folder that already has files in it into a workspace; witdo
asks first, and never moves, renames or changes the files that are there.

To open a workspace, pick it from the profile's recent list when witdo starts
(or with **Switch…** in the desktop app, `w` in the terminal app), or type a
folder's path: a folder that is not a workspace yet becomes one. *Forget*
removes a workspace from the list and leaves its files alone.

Deleting `.witdo/tmp/` or the profile's configuration never loses tasks; witdo
recreates what it needs.

## Profiles

A **profile** is a separate set of witdo's own state: the workspaces it knows
about, the one you opened last, and window preferences. Profiles share
nothing, so a "work" profile never shows where your "personal" workspaces are.

```sh
witdo --profile work                 # or WITDO_PROFILE=work
witdo --workspace ~/Notes/tasks      # open a folder and remember it
```

The default profile is called `default`. Profile state is stored in your
configuration folder, for example `~/.config/witdo/profiles/<name>/` on Linux.
It contains only folder paths and preferences, never task content.
