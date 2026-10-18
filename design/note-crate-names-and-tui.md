# Crate names and the terminal UI

Amends [plan.v1.md §10](plan.v1.md#10-technical-architecture). Decided 2026-10-18.

## `witdo-store` is now `witdo-io`

The crate that owns all workspace file I/O is called `witdo-io`. Its job is unchanged: Markdown
parse/serialize, workspace layout, scan, safe writes, trash and watching, and it stays the only
code that touches the filesystem (§9).
