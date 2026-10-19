---
name: render-check
description: Use when kiri's output needs a look in a real kitty window and nobody can watch one — after changing rendering of headings, images, tables, colours or anything else visual, before asking the maintainer for a visual check, or when asked to "render-check" a fixture. Renders Markdown documents in kitty on a virtual X display (Xvfb) and saves screenshots to read and describe.
---

# render-check

Renders documents with kiri inside kitty on a virtual X display of its own
and saves a screenshot of each. The agent's own terminal and the
maintainer's desktop are never touched, so it works unattended, also under
Wayland where screenshots of other windows need the user's consent.

## Running

From the repository root:

```sh
.claude/skills/render-check/render-check.sh tests/fixtures/images.md
.claude/skills/render-check/render-check.sh --out DIR a.md b.md -- --mode ansi
```

- It builds `target/debug/kiri` first, unless `--kiri PATH` is given.
- kiri may take 120 seconds per document (`--timeout SECONDS`). A debug
  build needs about 13 seconds for `examples/visual-check-images.md` on an
  idle machine and 30 on a busy one.
- Screenshots go to `.local/render-check/BRANCH/` by default (`--out DIR`),
  one `DATE-NAME.png` per document, such as `2026-10-18-tables.png`,
  cropped below the last drawn row. Taller screenshots are also sliced into
  `DATE-NAME-p01.png`, `DATE-NAME-p02.png`, … of 1600 pixels
  (`--page-height`), small enough to read one at a time. `.local` is shared
  with jailbee containers, so screenshots taken in one show up on the host.
- Arguments after `--` go to kiri. Without them kiri runs in `auto` mode
  and detects kitty the way it does for the user. kiri starts only when
  kitty answers a query, so its probe does not time out while kitty is
  still starting, which would show images as alt text.
- The window is 1280 × 16000 pixels (`--size WxH`): wide enough for the
  default text width and tall enough that most documents fit without
  scrolling. A narrower width checks wrapping and image shrinking. If a
  document is taller than the window, the script says so; the screen
  cannot be taller than about 16384 pixels.
- kiri prints the document instead of paging it (`KIRI_PAGING=never`),
  whatever the configuration file says, as nobody can press a key in the
  pager. The script cannot check the pager.
- The screenshot is taken once kitty shows the cursor where kiri's output
  ended and two screenshots in a row are equal. kitty's software renderer
  can take seconds to draw large images, and until then shows its last
  frame, which may look finished. A document that fills the screen
  scrolls, which weakens that check.
- kitty starts with its built-in defaults (`--config NONE`, 11 pt
  `monospace` font, 96 dpi, so a cell is about 9 × 20 pixels). Pass
  `--kitty-config FILE` to see the document with another configuration.

## Reviewing

Read every screenshot and say what is on it, element by element: sizes and
positions of images, wrapping, indentation, colours, anything cut off or
overlapping. Compare with what a browser would show; that is kiri's goal.
kiri's warnings on stderr appear at the top of the screenshot.

If kiri exits with an error or crashes, the script says so, still saves
that document's screenshot with the error on it, goes on with the other
documents and then exits with kiri's status, 128 plus the signal number
for a crash. Treat that as a failed check. When kitty does not answer its
first query within 60 s, or a screenshot cannot be compared, the script
stops at once with status 1 and starts no more documents. It stops the same
way when kiri leaves kitty's answers unread: kiri's probe timed out, which
happens on a busy machine, such as a new jailbee container still running
its background `make build`. Run it again once the machine is idle.

Report what you could not judge. The screenshots come from kitty's software
renderer (Mesa llvmpipe) with default fonts, so font rendering, colours of
the maintainer's theme and HiDPI scaling still need a look in the real
window.

## Requirements and safety

- `Xvfb` (Fedora: `xorg-x11-server-Xvfb`), `kitty`, ImageMagick (`import`,
  `magick`) and Mesa's llvmpipe for OpenGL. The script names what is
  missing; run it where they are installed: the host, or a jailbee
  container, whose golden image has them.
- Never copy over or edit font files, kitty's configuration or anything else
  a running kitty may have open; the maintainer's session lives in kitty.
  The script only reads them.
- The script stops its Xvfb and kitty on exit and removes its temporary
  directory.
