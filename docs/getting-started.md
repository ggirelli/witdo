# Getting started

There are no downloads yet: for now, witdo is built from source.

## Building witdo

You need [Rust](https://rustup.rs) and [`just`](https://github.com/casey/just).
`rustup` installs the Rust version witdo pins on the first build.

On Linux, the desktop app also needs the WebKitGTK and libsoup development
packages and `xdo`. The terminal app needs nothing extra.

=== "Debian and Ubuntu"

    ```sh
    sudo apt install libwebkit2gtk-4.1-dev libsoup-3.0-dev libxdo-dev build-essential pkg-config
    ```

=== "Fedora"

    ```sh
    sudo dnf install webkit2gtk4.1-devel libsoup3-devel libxdo-devel
    ```

Then:

```sh
git clone https://github.com/ggirelli/witdo
cd witdo
just run   # the desktop app
just tui   # the terminal app; q or Esc quits
```

## Two ways to use it

- **Desktop app** (`witdo`): a window with your views on the left, the task
  list in the middle and the selected task on the right.
- **Terminal app** (`witdo-tui`): the same tasks in a terminal, handy over SSH.

Both read and write the same files, so you can switch between them freely.

## The first time

witdo asks for a folder to keep your tasks in. Type its path; if the folder
does not exist or is empty, witdo makes it a workspace straight away, and if
it has files in it, witdo asks first. Next time, witdo opens it again.

Then add a task, and read [Using witdo](using-witdo.md) for the rest.
