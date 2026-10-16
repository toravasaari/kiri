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
- Screenshots go to `target/render-check/` by default (`--out DIR`), one
  `NAME.png` per document, cropped below the last drawn row. Taller
  screenshots are also sliced into `NAME-p01.png`, `NAME-p02.png`, … of
  1600 pixels (`--page-height`), small enough to read one at a time.
- Arguments after `--` go to kiri. Without them kiri runs in `auto` mode
  and detects kitty the way it does for the user.
- The window is 1280 × 16000 pixels (`--size WxH`): wide enough for the
  default text width and tall enough that most documents fit without
  scrolling. A narrower width checks wrapping and image shrinking. If a
  document is taller than the window, the script says so; the screen
  cannot be taller than about 16384 pixels.
- kitty starts with its built-in defaults (`--config NONE`, 11 pt
  `monospace` font, 96 dpi, so a cell is about 9 × 20 pixels). Pass
  `--kitty-config FILE` to see the document with another configuration.

## Reviewing

Read every screenshot and say what is on it, element by element: sizes and
positions of images, wrapping, indentation, colours, anything cut off or
overlapping. Compare with what a browser would show; that is kiri's goal.
kiri's warnings on stderr appear at the top of the screenshot.

Report what you could not judge. The screenshots come from kitty's software
renderer (Mesa llvmpipe) with default fonts, so font rendering, colours of
the maintainer's theme and HiDPI scaling still need a look in the real
window.

## Requirements and safety

- `Xvfb` (Fedora: `xorg-x11-server-Xvfb`), `kitty`, ImageMagick (`import`,
  `magick`) and Mesa's llvmpipe for OpenGL. The script names what is
  missing; run it where they are installed, usually the host.
- Never copy over or edit font files, kitty's configuration or anything else
  a running kitty may have open; the maintainer's session lives in kitty.
  The script only reads them.
- The script stops its Xvfb and kitty on exit and removes its temporary
  directory.
