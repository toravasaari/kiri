# Diagrams

kiri shows a diagram written in a code block as an image in the
block's place, and a diagram file given on its own as an image.

A code block whose language kiri draws, `mermaid` for now, is shown as a
diagram, and so is a [formula](formulas.md), which kiri draws through
the same pipeline. The first of these that is given decides whether it
is:

1. `--diagrams on|off`
2. `KIRI_DIAGRAMS`: `on` or `1`, `off` or `0`; an empty value counts as
   unset, and any other value is left out with a warning
3. `enabled = true` or `false` in the `[diagrams]` table of the
   configuration file, or `diagrams = true` or `false`
4. on

Off shows diagrams and formulas as code. They are drawn as images, so with
[images](config.md#images) off or in a terminal without kitty's graphics
they stay code whatever this says.

What draws a Mermaid diagram:

- In the default, full build, kiri itself, through
  [merman](https://crates.io/crates/merman): every diagram type, with no
  program to install and no network.
- In a build without default features, mermaid-cli's `mmdc`, when kiri
  finds it: on `PATH`, then in npm's global `bin` directory
  (`NPM_CONFIG_PREFIX`, or `prefix` in `~/.npmrc`), in the `bin`
  directories of nvm's Node versions, the newest first (`NVM_DIR`, or
  `~/.nvm`), and in Homebrew's `/opt/homebrew/bin` and
  `/home/linuxbrew/.linuxbrew/bin`. A program found off `PATH` runs with
  its own directory first on `PATH`, so that it finds the `node` beside
  it. kiri runs it with the command line shown under
  [Diagram commands](#diagram-commands). Without one, diagrams stay
  code. `auto_detect = false` in the `[diagrams.mermaid]` table keeps
  kiri from looking for `mmdc`.
- In either build, the command the configuration file gives. kiri then
  neither draws the language itself nor looks for `mmdc`.

A diagram that sets its own theme, in a `%%{init: {"theme": …}}%%`
directive or under `config:` in its front matter, keeps it. Otherwise it
takes Mermaid's `default` theme on a light page and `dark` on a dark one.
The page is light with `--light` (the light paper theme), dark with `--dark`, and otherwise as
the terminal's background is; an unknown background counts as dark.

A diagram's own colours are for a dark page in Mermaid's dark mode,
`darkMode: true` among its theme variables, and otherwise when its
background is dark: the `background` it sets, or else its theme's, as
`dark`, `neo-dark`, `redux-dark` and `redux-dark-color` have. A diagram
whose own theme is for the other page is drawn on a panel, so that its
lines and text show: one in light colours on a dark page on a
white panel, and one in dark colours on a light page on a panel of its
theme's background. When kiri cannot tell that background, as with an
external program, the panel is Mermaid's dark grey, `#333333`.
`background_panel = false` in the `[diagrams]` table, `--diagram-panel off`
or `KIRI_DIAGRAM_BACKGROUND_PANEL=off` leaves the panels out.

A diagram that does not draw, such as one with a syntax error, stays
code. Below the code, in the theme's `error` colour, kiri tells why, as
in `✗ mermaid diagram could not be drawn: …`, which shows in the pager
too. The lines start at the edge of the code's box and are no wider than
the code in it; without a box, they start where the code does. A warning
on standard error names the diagram by its line, as in
`kiri: mermaid diagram on line 12: …`, or by its first line for a
document piped in. [Render errors](config.md#render-errors) choose either
one alone.

A Mermaid file given on its own, `kiri plan.mmd`, `.mermaid` or
`--format mermaid`, is drawn with these same settings and shown as large
as fits the window, as an image file is; unlike a document's diagrams,
it is drawn with images off too. When it is not drawn, its source is
shown as `--format text` shows it; one that fails to draw is an error,
told below its source and as `kiri: plan.mmd: …` on standard error, or
in one of them as [render errors](config.md#render-errors) say, and kiri
exits with status 1 either way. One that nothing can draw, although diagrams
are on and the terminal shows them, gets the hint
`kiri: plan.mmd: no program to draw Mermaid diagrams; see --doctor`.

`kiri --doctor` shows the switch, the time limit, whether panels are
on and where render errors go, each with where it comes from, and
lists, for each language, the renderer built into this build and the
external command, configured, found, not found or not looked for, and
which of them is used.

## Diagram commands

A table under `diagrams` for a language gives the program that draws its
diagrams and the program's arguments:

```toml
[diagrams.mermaid]
command = ["mmdc", "-i", "{input}", "-o", "{output}", "-t", "{theme}", "-b", "transparent", "-c", "{config}", "-s", "{scale}"]
```

This is mermaid-cli's command line, the same one kiri runs when it finds
`mmdc` by itself. The table's language may be written in any case, as a
code block's is: `[diagrams.Mermaid]` is the same table, and a file with
both is an error. `mmdc` draws the diagram in its browser and writes a
PNG of it, three pixels to the CSS pixel, which kiri shrinks to the size
it shows the diagram at; the diagram then looks as it does in a browser.
kiri writes the diagram's source to a file in a directory of its own,
runs the program directly, without a shell, and reads the image it
writes. The directory is in `diagrams` in kiri's cache directory
(`$XDG_CACHE_HOME/kiri`, or `~/.cache/kiri`), made for you alone when it
is needed, or in the temporary directory (`TMPDIR`, or `/tmp`) when that
cannot be made or its path is longer than 49 bytes, too long for the
sockets a browser may make in it. The program gets the directory as its
`TMPDIR` too, so that what it leaves there, such as the profile of
`mmdc`'s browser, goes with the directory once the diagram is drawn. A
directory left in `diagrams` by a kiri that did not end normally, such
as one killed with SIGKILL, goes the next time kiri draws there. Each
directory has a file, `host`, with the name of the machine it was made
on, and only a kiri on that machine removes it, so that machines or
containers can share the cache directory. `kiri --doctor` shows these
directories under *Diagram work files*, and why the cache directory
cannot be used when it cannot. In the arguments:

- `{input}` is the file of the diagram's source, which ends in `.mmd`
  for Mermaid;
- `{output}` is the image file the program writes: SVG, or a raster
  image such as PNG, which kiri tells apart by its content. kiri reads
  at most 100 MB of it, whatever `--max-image-size` says, which limits
  image files only. Its name ends in `.png` when the command has
  `{scale}`, and in `.svg` otherwise, so that a program such as `mmdc`,
  which writes the format its output file's name ends in, writes the one
  kiri expects;
- `{theme}` is the theme kiri chose: `default` or `dark` for Mermaid;
- `{scale}` is `3`, the number of pixels to the CSS pixel that the
  program is to draw a raster image with. kiri shows such an image at a
  third of its size in pixels, in CSS pixels as it shows an SVG, and
  keeps no more of its pixels than it can show: as many cells as one
  image can cover, wide and high. A PNG with more than 64 megapixels,
  more than kiri loads, as a long sequence diagram may have, is drawn
  once more at a smaller scale, `2` or `1`, the largest that fits; kiri
  tells from the PNG's header, before loading it, and shows the second
  image at the same size in CSS pixels. The second drawing has a time
  limit of its own. A diagram too large even then stays code, and kiri
  tells its size in pixels as it tells why any diagram did not draw:
  below the code, in a warning or both, as
  [Render errors](config.md#render-errors) says. A raster image from a
  command without `{scale}` is shown at one pixel to the CSS pixel;
- `{config}` is a Mermaid configuration file, in JSON, that has each
  diagram drawn at its own width, rather than at most as wide as the
  800-pixel page of `mmdc`'s browser, and the classic look and the dagre
  layout that kiri's built-in renderer uses too; a diagram's own
  settings win over it. For a command without `{scale}`, which is taken
  to write SVG, it also has labels drawn as SVG text rather than as
  HTML, which kiri cannot draw in an SVG.

`{input}` and `{output}` must be there. The source file is the diagram
as the document has it, so line numbers in the program's errors are the
diagram's own. A command without `{config}` works too, but `mmdc` then
draws a wide diagram narrower, and when it writes SVG, the labels of
some diagram types, such as flowcharts, as HTML, which may show blank.
No shell expands the arguments, so a leading `~/` is not your home
directory: write the path in full. A program named without a path is
looked for on `PATH`.

Each diagram may take 20 seconds, or as long as `timeout` in the
`[diagrams]` table says, such as `timeout = 60` or `timeout = 2.5`, or
`--diagram-timeout` or `KIRI_DIAGRAM_TIMEOUT`; up
to four are drawn at once. When drawing takes longer than half a second,
a line such as `kiri: loading images 2/5…` on the terminal tells how
far it has got, and is wiped before the document is shown. Ctrl-C,
Ctrl-\\, SIGTERM or SIGHUP meanwhile stops the programs and removes their
files, and then the signal ends kiri as it would have otherwise, so that
a shell shows status 130 after Ctrl-C. A signal that was ignored when
kiri started, as under `nohup`, stays ignored. A program that fails,
runs out of time or writes no image leaves its diagram as code with a
warning; kiri does not fall back to drawing it itself, so that a command
that does not work shows. The warning, and the error below the code,
quote the first lines the program wrote on standard error, at most five
and none of a stack trace, each keeping its indent, so that mmdc's
pointer to a mistake shows under its line.

In the pager, `r` and following draw the diagrams that changed in the
background instead: the text shows at once, a changed diagram shows its
old drawing dimmed until the new one comes, and the status line says
`[rendering: 1/3]` in place of the file name. `q` stops the programs
still drawing. With `initial_load = "background"`, the pager opens without
waiting for the diagrams, each shown as its code until it is drawn; see
[Initial load](config.md#initial-load).

kiri stops a program as Ctrl-C would, with SIGINT to its process group,
and kills what is left of the group half a second later. `mmdc` closes
its browser on SIGINT; the browser runs outside the group, so killing
`mmdc` alone would leave it running.

With a command configured, `enabled = false` in the `[diagrams]` table,
`--diagrams off` or `KIRI_DIAGRAMS=0` turns diagrams off.
