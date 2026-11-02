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

- Everything committed is in English: code, comments, docs, commit messages.

## Source of truth

- The code and its tests describe current behaviour. Docs describe intent and
  rationale; when they disagree with the code, say so instead of guessing.
- Plans and specs are working notes. Keep them out of the committed tree,
  and never reference them from committed files.

## Architecture rules

- The pipeline is parse -> document model -> backend. Parsing knows nothing
  about terminals; backends know nothing about Markdown syntax.
- Every kitty-specific escape sequence lives under `src/present/kitty/`,
  which the placement map in docs/architecture.md puts in the terminal layer.
  Other backends (HTML, pixel rendering) and terminals without kitty's
  protocols must stay possible without touching the parser or the document
  model.
- Rendering writes to a `std::io::Write` and takes terminal capabilities as
  input. Capability detection and other terminal I/O happen in one place
  (`src/term/`), so renderers stay pure and testable.
- No global mutable state.

## Placement

- New code goes where the placement map in docs/architecture.md puts it.
  When the map has no place for it, stop and ask the maintainer; do not add
  it to a hub file.
- A reviewer checks the placement of new code against the map.

## Commands

Run everything through `make` so local runs match CI:

- `make check` runs formatting check, clippy with warnings as errors, and the
  test suite. This is the definition of done.
- `make test` runs the suite with cargo-nextest when it is installed, and
  with `cargo test` otherwise.
- `make build` compiles everything `make check` needs without running
  it, so a later `make check` finds most of the build done.
- `make sweep` deletes compiled crates and test binaries in `target/` that
  cargo has not built or checked for 7 days, and then the oldest ones until
  `target/` is under 100 GB. `make check` runs it after a passing run; it
  skips itself in a container or without `cargo-sweep`.
- `make fmt` formats the code.
- Run `make check` in the foreground. A job started in the background of
  a non-interactive shell, as `( make check & )` or `nohup make check`,
  inherits SIGINT as ignored, and a few tests of signals and stopping
  programs then fail although the code is fine.
- `make release` builds the optimised binary, `target/release/kiri`.
- `cargo run -- <file.md>` renders a document in the current terminal.
- A build outside the main checkout, such as a scratch copy for a review,
  uses a `CARGO_TARGET_DIR` of its own, never the checkout's `target/`, and
  removes it when done. Test binaries bake in their source path, so a shared
  `target/` ends up running stale binaries.

[`.jailbee/`](.jailbee/README.md) configures isolated containers for agent
work.

## Testing

- Renderer output is tested with snapshot tests (`insta`). The snapshot is
  the escape-sequence stream, so tests run without a terminal.
- Review new or changed snapshots before accepting them; a snapshot that
  nobody looked at is not a test.
- Fixes and behaviour changes start with a test that fails on the current
  code. Show that failing run before making the change. The run must fail
  on an assertion, not only on a compile error, wherever possible: stub the
  new code first if needed. The test and the change go in one commit, so
  every commit still passes `make check`. The history cannot show the
  failing run, so show it in the session, or in your report when nobody is
  watching.
- Before rewriting code, pin its current behaviour with tests that pass on
  the old code, then keep them passing.
- Fixture documents live in `tests/fixtures/`. Add one when you add support
  for a Markdown element.
- A change in how some part of a document looks gets an example in the
  topic document in `examples/` it belongs to, with a sentence on what one
  should see there. Do it as part of the feature, without being asked: the
  maintainer checks new work by looking at the examples. A change every
  document shows alike, such as pager scrolling, gets none; the report says
  where to see it.
- Visual output still needs a look in a real kitty window. Say so when you
  could not do that check. For a first look, the render-check skill
  (`.agents/skills/render-check/`) screenshots documents rendered in kitty
  on a virtual display.

## Review and contributions

- Code is reviewed by a different agent running a different model from
  the one that wrote it.
- Agents work on their own branch, one commit per logical change. Review
  fixes go in as ordinary commits on the branch. Agents never rewrite
  history; only the maintainer may rebase an unfinished branch.
- A small review of every commit before the merge, and a larger one when a
  part of the [roadmap](docs/roadmap.md) is finished.
- Reviewers never change the working tree or git state; experiments go in
  a scratch copy.
- Fix the clear findings, a bug with one obvious fix or a missing test,
  and report them as fixed. Ask the maintainer about the rest: findings
  that are unclear, or that change agreed behaviour or the plan.
- Pull requests from outside are not accepted yet.

## Commits

- Conventional Commits: `feat:`, `fix:`, `docs:`, `test:`, `refactor:`,
  `perf:`, `chore:`, `style:`, with an optional scope such as `feat(kitty):`.
- Keep messages clear, easy to understand and short. Subject in the
  imperative. A body only when the reason is not obvious; longer text is for
  exceptional cases.
- Wrap lines at about 80 characters. Never break URLs or code.
- One logical change per commit, so history stays bisectable.
- Mark AI-assisted commits with an `Assisted-by` trailer naming the agent
  and the exact model in use, in the Linux kernel format:

  ```
  Assisted-by: Claude:claude-opus-5-5
  ```

- Use `Assisted-by` instead of `Signed-off-by` or `Co-Authored-By`
  trailers on AI-assisted commits; do not add those.
- Commits are free; pushes are not. Anything that changes a remote needs the
  maintainer's explicit approval, every time.

## Reporting

Report failures as failures and skipped steps as skipped. If `make check`
fails, show the output instead of summarising it away.
