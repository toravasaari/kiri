# Configuration

kiri takes its settings from the command line, from environment variables
and from a configuration file. The command line is the strongest, then the
environment, then the file.

## Configuration file

kiri reads `$XDG_CONFIG_HOME/kiri/config.toml`, or
`~/.config/kiri/config.toml` when `XDG_CONFIG_HOME` is unset, empty or
not an absolute path. The
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

# Whether the pager follows a lone file, reading it again whenever it
# changes: "on", "off" or "auto", which also moves the view to a change.
follow = "off"

# Front matter render style: "table", "code", "hide" or "off".
front_matter = "table"

# Table row style: "zebra", "lines", "indent", "underline" or "off".
table_rows = "zebra"

# Where kiri tells why a diagram could not be drawn: "inline", "stderr"
# or "both"; see Render errors below.
render_errors = "both"

# Diagrams in code blocks, such as Mermaid; see Diagrams below.
[diagrams]
# false shows them as code.
enabled = true
# How long a program may take to draw one diagram, in seconds.
timeout = 20
# false draws no panel under a diagram in colours for the other page.
background_panel = true

# A program that draws Mermaid diagrams instead of kiri; see
# Diagram commands in diagrams.md.
[diagrams.mermaid]
command = ["mmdc", "-i", "{input}", "-o", "{output}", "-t", "{theme}", "-b", "transparent", "-c", "{config}", "-s", "{scale}"]
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
alt text, without warnings about images that would not have loaded, and
[diagrams](diagrams.md) in code blocks stay code. These settings are about
images inside documents: an image or Mermaid file given on the command
line, `kiri photo.jpg` or `kiri plan.mmd`, is shown whatever they say.

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

`auto` pages a document longer than the window; a shorter one is printed,
unless it is a lone file that is [followed](#follow).
A document from standard input, often another command's output, and the
`--doctor` report are paged only with `always`, and so is an image or a
diagram file, on its own, as in `kiri -p plan.mmd`, or with others that
are all images or diagram files; with a document among them, the setting
applies to all the files. Whatever the setting, kiri prints the
document when its output is not a terminal or `--mode plain` asks for
text without escape sequences.

## Follow

The first of these that is given decides whether the pager follows a lone
file: `on` reads it again whenever it changes, with the view kept at the
same block, as `r` reads it again; `auto` also moves the view to a change;
`off` does not follow it:

1. `--follow on|off|auto`, `-f` for `on`, `--fa` for `auto` or `-F` for
   `off`; only one of them can be given
2. `KIRI_FOLLOW`: `on`, `off` or `auto`, `1` for `on` and `0` for `off`;
   an empty value counts as unset, and any other value is left out with a
   warning
3. `follow` in the configuration file
4. `off`

Following pages a lone file as `-p` does, also when it is shorter than
the window, unless paging is `never`: `kiri -f -P notes.md` prints it.
`F` in the pager goes from `off` to `on` to `auto` and back to `off`, and
the status line says `F follow` while following is on and `F auto` while
it is auto. Following notices a change by the file's
modification time and length, so on a filesystem that keeps the time only
to the second, an edit within the same second that keeps the length waits
for the next change. A document from standard input, or several files,
cannot be followed yet: with `-f`, `--fa`, `--follow on` or `--follow auto` the pager says
`nothing to follow`, and with `KIRI_FOLLOW` or `follow` in the
configuration file it says nothing.

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

Which diagrams kiri draws, and with what, is in [Diagrams](diagrams.md).

## Render errors

The first of these that is given decides where kiri tells why a diagram
could not be drawn:

1. `--render-errors inline|stderr|both`
2. `KIRI_RENDER_ERRORS` with the same values, as for `KIRI_FRONT_MATTER`
3. `render_errors` in the configuration file
4. `both`

`inline` tells it only in the document, below the diagram's code, in the
theme's `error` colour, or without colour where there are none;
`stderr` only in a warning on standard error; `both` in both. Warnings
about images that do not load go to standard error whatever this says.

## Environment variables

| Variable | Meaning |
|---|---|
| `KIRI_CONFIG` | The configuration file to read instead of the default |
| `KIRI_THEME` | A theme file; see [Theme](#theme) |
| `KIRI_NO_IMAGES` | Images as alt text; see [Images](#images) |
| `KIRI_PAGING` | When to page; see [Paging](#paging) |
| `KIRI_FOLLOW` | Whether the pager follows a file; see [Follow](#follow) |
| `KIRI_FRONT_MATTER` | Front matter render style; see [Front matter](#front-matter) |
| `KIRI_TABLE_ROWS` | Table row style; see [Table rows](#table-rows) |
| `KIRI_DIAGRAMS` | Diagrams drawn or shown as code; see [Diagrams](diagrams.md) |
| `KIRI_RENDER_ERRORS` | Where kiri tells why a diagram was not drawn; see [Render errors](#render-errors) |
| `NO_COLOR` | No colours, when set and not empty; see [no-color.org](https://no-color.org) |
| `HOME` | Your home directory, `~`, for the default configuration and cache directories, a theme under `~/` and programs found under it; ignored when empty or not an absolute path |
| `XDG_CONFIG_HOME` | Where the configuration directory is; `~/.config` when unset or not an absolute path |
| `XDG_CACHE_HOME` | Where kiri's cache directory is, for the files of [diagram commands](diagrams.md#diagram-commands); `~/.cache` when unset or not an absolute path |
| `TMPDIR` | The temporary directory, for the files of [diagram commands](diagrams.md#diagram-commands) when the cache directory cannot be used; `/tmp` when unset or empty |
