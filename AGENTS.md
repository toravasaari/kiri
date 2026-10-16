# AGENTS.md

Instructions for coding agents working on kiri. Read this file in full before
your first edit. Human-facing documentation starts at [README.md](README.md).

## Project

kiri renders Markdown documents in the kitty terminal so that they look close
to what a browser would show: real large headings through kitty's text sizing
protocol, images, formulas and diagrams through the graphics protocol, and
clickable links. It is written in Rust and ships as a single binary, `kiri`.

- Direction and phases: [docs/roadmap.md](docs/roadmap.md)
- Design and module boundaries: [docs/architecture.md](docs/architecture.md)

## Language

- Discussion with the maintainer is in Finnish.
- Everything committed is in English: code, comments, docs, commit messages.

## Source of truth

- The code and its tests describe current behaviour. Docs describe intent and
  rationale; when they disagree with the code, say so instead of guessing.
- Plans and specs are working notes. Keep them in the gitignored `.local/`
  directory, not in the tree, and never reference them from committed files.

## Architecture rules

- The pipeline is parse -> document model -> backend. Parsing knows nothing
  about terminals; backends know nothing about Markdown syntax.
- Every kitty-specific escape sequence lives under `src/kitty/`. Other
  backends (plain ANSI, HTML, pixel rendering) must stay possible without
  touching the parser or the document model.
- Rendering writes to a `std::io::Write` and takes terminal capabilities as
  input. Capability detection and other terminal I/O happen in one place
  (`src/term.rs`), so renderers stay pure and testable.
- No global mutable state.

## Commands

Run everything through `make` so local runs match CI:

- `make check` runs formatting check, clippy with warnings as errors, and the
  test suite. This is the definition of done.
- `make fmt` formats the code.
- `make release` builds the optimised binary, `target/release/kiri`.
- `cargo run -- <file.md>` renders a document in the current terminal.

Isolated agent work runs in [jailbee](https://jailbee.gisgro.io)
containers; `.jailbee/` holds the configuration and the rustup install step.
The network is strict there: crates.io and rustup work, GitHub does not.

## Testing

- Renderer output is tested with snapshot tests (`insta`). The snapshot is
  the escape-sequence stream, so tests run without a terminal.
- Review new or changed snapshots before accepting them; a snapshot that
  nobody looked at is not a test.
- Fixes and behaviour changes start with a test that fails on the current
  code. Show that failing run before making the change. The run must fail
  on an assertion, not only on a compile error, wherever possible: stub the
  new code first if needed.
- Before rewriting code, pin its current behaviour with tests that pass on
  the old code, then keep them passing.
- Fixture documents live in `tests/fixtures/`. Add one when you add support
  for a Markdown element.
- Visual output still needs a look in a real kitty window. Say so when you
  could not do that check.

## Review

- Code is reviewed by a different agent running a different model from
  the one that wrote it.
- A small review before every commit, and a larger one when a part of the
  [roadmap](docs/roadmap.md) is finished.
- Report the findings to the maintainer; fix only what is agreed.

## Commits

- Conventional Commits: `feat:`, `fix:`, `docs:`, `test:`, `refactor:`,
  `chore:`, `style:`, with an optional scope such as `feat(kitty):`.
- Keep messages clear, easy to understand and short. Subject in the
  imperative. A body only when the reason is not obvious; longer text is for
  exceptional cases.
- Wrap lines at about 80 characters. Never break URLs or code.
- One logical change per commit, so history stays bisectable.
- Agents mark their commits with an `Assisted-by` trailer naming the agent
  and the exact model in use, in the Linux kernel format:

  ```
  Assisted-by: Claude:claude-opus-5-5
  ```

- Never add `Signed-off-by`, `Co-Authored-By` or `Claude-Session` lines.
- Commits are free; pushes are not. Anything that changes a remote needs the
  maintainer's explicit approval, every time.

## Reporting

Report failures as failures and skipped steps as skipped. If `make check`
fails, show the output instead of summarising it away.
