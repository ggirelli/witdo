# Contributing to witdo

## Making a change

1. Keep changes focused: one coherent step per change, each building and passing the checks.
2. Run the checks CI runs:

   ```sh
   just check   # rustfmt, clippy (pedantic, warnings are errors), tests, cargo-deny
   ```

3. Add or update tests. Domain rules (`witdo-core`) and storage rules (`witdo-io`) are tested
   before any UI depends on them.
4. Update the docs that describe what you changed: [README.md](README.md) for people using witdo,
   `design/` for how it works (new decisions go in a new `design/plan.vN.md` or a focused note),
   and an entry in [CHANGELOG.md](CHANGELOG.md) under "Unreleased".

## Style

- Code follows `rustfmt` and `clippy::pedantic`; `unsafe` is forbidden. Prefer clear names over
  comments, and comment the *why* when it is not obvious.
- Respect the crate boundaries: `witdo-core` knows nothing about files or the UI, all workspace
  file I/O goes through `witdo-io`, and the UI calls `witdo-app` only.
- Errors are actionable and never swallowed. Logs never include task titles or notes.
- Messages shown to people are short, friendly sentences in British English.
- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/)
  (`feat:`, `fix:`, `docs:`…), with a body explaining what changed and why.

## Dependencies

`cargo deny` checks advisories and licences. Every exception in [deny.toml](deny.toml) says
which dependency needs it and why; keep it that way, and drop exceptions once they stop matching.

## Assets

Brand artwork lives in `assets/brand/`. Only the compressed copies are committed; full-size
originals stay local and are listed in `.gitignore`.

By contributing, you agree that your contributions are licensed under the [MIT licence](LICENSE).
