# Configuration

kiri takes its settings from the command line, from environment variables
and from a configuration file. The command line is the strongest, then the
environment, then the file.

## Configuration file

kiri reads `$XDG_CONFIG_HOME/kiri/config.toml`, or
`~/.config/kiri/config.toml` when `XDG_CONFIG_HOME` is unset or empty. The
file is optional. `KIRI_CONFIG` names another file instead, and that file
must exist. A mistake in the file, an unknown key among them, is an error,
not silently ignored.

Every key is optional:

```toml
# A theme file. A relative path is relative to the directory of this
# file, and a leading ~/ is your home directory.
theme = "themes/mine.toml"

# false shows every image as its alt text, without reading it.
images = true

# When to show the document in the pager: "always", "auto" or "never".
paging = "auto"

# How a search in the pager treats case: "smart" or "literal".
search_case = "smart"

# Front matter render style: "table", "code", "hide" or "off".
front_matter = "table"

# Table row style: "zebra", "lines", "indent", "underline" or "off".
table_rows = "zebra"

# Diagrams in code blocks, such as Mermaid; see Diagrams below.
[diagrams]
# false shows them as code.
enabled = true
# How long a program may take to draw one diagram, in seconds.
timeout = 20
# false draws no panel under a diagram in colours for the other page.
background_panel = true

# A program that draws Mermaid diagrams instead of kiri; see
# Diagram commands below.
[diagrams.mermaid]
command = ["mmdc", "-i", "{input}", "-o", "{output}", "-t", "{theme}", "-b", "transparent", "-c", "{config}"]
# false: a build without default features does not look for mmdc.
auto_detect = true
```

`diagrams = false` on its own, among the keys above the tables, is short
for `enabled = false` in the `[diagrams]` table. A file that has it
cannot have the `[diagrams]` table or a table under it, such as
`[diagrams.mermaid]`.

## Theme

The first of these that is given chooses the [theme](theme.md):

1. `--theme FILE`
2. `--light` or `--dark`, the [paper themes](theme.md#paper-themes)
3. `KIRI_THEME`, when it is not empty
4. `theme` in the configuration file
5. `theme.toml` in the configuration directory
   (`$XDG_CONFIG_HOME/kiri/theme.toml` or `~/.config/kiri/theme.toml`),
   when it exists
6. the built-in theme

A file named by `--theme`, `KIRI_THEME` or the configuration file must
exist. `--light` and `--dark` cannot be given together. Even when another
theme wins, they choose the code colours for a light or a dark background
instead of the terminal's background.

## Images

Images are shown by default. The first of these that is given decides
instead:

1. `--images` turns them on, `-I` or `--no-images` off; the two cannot be
   given together
2. `KIRI_NO_IMAGES`: any value but `0` turns images off, and `0` turns them
   on whatever the configuration file says; an empty value counts as unset
3. `images` in the configuration file

With images off, kiri reads no image files and shows every image as its
alt text, without warnings about images that would not have loaded.
These settings are about images inside documents: an image file given on
the command line, `kiri photo.jpg`, is shown whatever they say.

## Paging

The first of these that is given decides when a document is shown in the
pager:

1. `--paging always|auto|never`, `-p` for `always` or `-P` for `never`;
   only one of them can be given
2. `KIRI_PAGING`: `always`, `auto` or `never`, `1` for `always` and `0` for
   `never`; an empty value counts as unset, and any other value is left
   out with a warning
3. `paging` in the configuration file
4. `auto`

`auto` pages a document longer than the window; a shorter one is printed.
A document from standard input, often another command's output, and the
`--doctor` report are paged only with `always`. Whatever the setting, kiri
prints the document when its output is not a terminal or `--mode plain`
asks for text without escape sequences.

## Search case

A search in the pager ignores case when its term has no upper-case letter,
and matches case exactly when it has one: `kiri` finds `Kiri` too, but
`Kiri` finds only `Kiri`. `search_case = "literal"` in the configuration
file makes every search match case exactly. Only the configuration file
sets this; there is no flag or environment variable for it.

## Front matter

The first of these that is given decides the front matter render style,
for the metadata block at the very start of a Markdown file:

1. `--front-matter table|code|hide|off`
2. `KIRI_FRONT_MATTER` with the same values; an empty value counts as
   unset, and any other value is left out with a warning
3. `front_matter` in the configuration file
4. `table`

`table` shows its keys and values as a table, `code` as a code block in
its language, `hide` not at all, and `off` treats the block as Markdown.
YAML, TOML or JSON that does not parse shows as a code block in the
table style, with a warning naming the line of the file, such as
`kiri: front matter, line 4: …`. CSV and TSV files have no front matter.

## Table rows

The first of these that is given decides the table row style of tables
drawn with borders:

1. `--table-rows zebra|lines|indent|underline|off`
2. `KIRI_TABLE_ROWS` with the same values, as for `KIRI_FRONT_MATTER`
3. `table_rows` in the configuration file
4. `zebra`

`zebra` shades every second row with the page colour mixed a little
towards the text colour. It needs 24-bit colour and the terminal's own
colours, which kitty and most modern terminals tell, or a page colour
from the theme, and draws nothing where it cannot have them. `lines`
draws a rule between the rows. `indent` starts the lines after the first
of a wrapped value two cells in. `underline` underlines each row's last
line, dashed, in the table border colour; it needs underline styles, as
kitty has them, and draws nothing elsewhere. `off` marks nothing. A
table too narrow for its columns is stacked as `Header: value` lines and
keeps that layout whatever this says. In plain output only `lines` and
`indent` mark the rows.

## Diagrams

A code block whose language kiri draws, `mermaid` for now, is shown as a
diagram. The first of these that is given decides whether it is:

1. `--diagrams on|off`
2. `KIRI_DIAGRAMS`: `on` or `1`, `off` or `0`; an empty value counts as
   unset, and any other value is left out with a warning
3. `enabled = true` or `false` in the `[diagrams]` table of the
   configuration file, or `diagrams = true` or `false`
4. on

Off shows diagrams as code. They are drawn as images, so with
[images](#images) off or in a terminal without kitty's graphics they
stay code whatever this says.

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
The page is light with `--light`, dark with `--dark`, and otherwise as
the terminal's background is; an unknown background counts as dark.

A diagram whose own theme is for the other page is drawn on a panel, so
that its lines and text show: one in light colours on a dark page on a
white panel, and one in dark colours on a light page on a panel of its
theme's background. When kiri cannot tell that background, as with an
external program, the panel is Mermaid's dark grey, `#333333`.
`background_panel = false` in the `[diagrams]` table leaves the panels
out.

A diagram that does not draw, such as one with a syntax error, stays
code with a warning that names it by its line, as in
`kiri: mermaid diagram on line 12: …`, or by its first line for a
document piped in.

A Mermaid file given on its own, `kiri plan.mmd`, `.mermaid` or
`--format mermaid`, is drawn with these same settings and shown as large
as fits the window, as an image file is; unlike a document's diagrams,
it is drawn with images off too. When it is not drawn, its source is
shown as `--format text` shows it; one that fails to draw is an error,
`kiri: plan.mmd: …`, and kiri exits with status 1.

`kiri --doctor` shows the switch, the time limit and whether panels are
on, each with where it comes from, and lists, for each language, the
renderer built into this build and the external command, configured,
found, not found or not looked for, and which of them is used.

### Diagram commands

A table under `diagrams` for a language gives the program that draws its
diagrams and the program's arguments:

```toml
[diagrams.mermaid]
command = ["mmdc", "-i", "{input}", "-o", "{output}", "-t", "{theme}", "-b", "transparent", "-c", "{config}"]
```

This is mermaid-cli's command line, the same one kiri runs when it finds
`mmdc` by itself. kiri writes the diagram's source to a file in a
directory of its own, runs the program directly, without a shell, and
reads the SVG it writes. The directory is in `diagrams` in kiri's cache
directory (`$XDG_CACHE_HOME/kiri`, or `~/.cache/kiri`), made for you
alone when it is needed, or in the temporary directory (`TMPDIR`, or
`/tmp`) when that cannot be made or its path is longer than 49 bytes,
too long for the sockets a browser may make in it. The program gets the
directory as its `TMPDIR` too, so that what it leaves there, such as the
profile of `mmdc`'s browser, goes with the directory once the diagram
is drawn. In the arguments:

- `{input}` is the file of the diagram's source, which ends in `.mmd`
  for Mermaid;
- `{output}` is the SVG file the program writes;
- `{theme}` is the theme kiri chose: `default` or `dark` for Mermaid;
- `{config}` is a Mermaid configuration file, in JSON, that has labels
  drawn as SVG text rather than as HTML, which kiri cannot draw.

`{input}` and `{output}` must be there. The source file is the diagram
as the document has it, so line numbers in the program's errors are the
diagram's own. A command without `{config}` works too, but `mmdc` then
draws the labels of some diagram types, such as flowcharts, as HTML,
and they may show blank. No shell expands the arguments, so a leading `~/`
is not your home directory: write the path in full. A program named
without a path is looked for on `PATH`.

Each diagram may take 20 seconds, or as long as `timeout` in the
`[diagrams]` table says, such as `timeout = 60` or `timeout = 2.5`; up
to four are drawn at once. When
drawing takes longer than half a second, a line such as
`kiri: rendering diagrams 2/5…` on the terminal tells how far it has
got, and is wiped before the document is shown. Ctrl-C, SIGTERM or
SIGHUP meanwhile stops the programs and removes their files, and then
the signal ends kiri as it would have otherwise, so that a shell shows
status 130 after Ctrl-C. A signal that was ignored when kiri started,
as under `nohup`, stays ignored. A program that fails,
runs out of time or writes no SVG leaves its diagram as code with a
warning; kiri does not fall back to drawing it itself, so that a
command that does not work shows.

kiri stops a program as Ctrl-C would, with SIGINT to its process group,
and kills what is left of the group half a second later. `mmdc` closes
its browser on SIGINT; the browser runs outside the group, so killing
`mmdc` alone would leave it running.

With a command configured, `enabled = false` in the `[diagrams]` table,
`--diagrams off` or `KIRI_DIAGRAMS=0` turns diagrams off.

## Environment variables

| Variable | Meaning |
|---|---|
| `KIRI_CONFIG` | The configuration file to read instead of the default |
| `KIRI_THEME` | A theme file; see [Theme](#theme) |
| `KIRI_NO_IMAGES` | Images as alt text; see [Images](#images) |
| `KIRI_PAGING` | When to page; see [Paging](#paging) |
| `KIRI_FRONT_MATTER` | Front matter render style; see [Front matter](#front-matter) |
| `KIRI_TABLE_ROWS` | Table row style; see [Table rows](#table-rows) |
| `KIRI_DIAGRAMS` | Diagrams drawn or shown as code; see [Diagrams](#diagrams) |
| `NO_COLOR` | No colours, when set and not empty; see [no-color.org](https://no-color.org) |
| `XDG_CONFIG_HOME` | Where the configuration directory is; `~/.config` when unset |
