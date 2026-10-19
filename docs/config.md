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
```

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
`--doctor` report are paged only with `always`. Whatever the setting, kiri prints the document when its output is not a
terminal or `--mode plain` asks for text without escape sequences.

## Environment variables

| Variable | Meaning |
|---|---|
| `KIRI_CONFIG` | The configuration file to read instead of the default |
| `KIRI_THEME` | A theme file; see [Theme](#theme) |
| `KIRI_NO_IMAGES` | Images as alt text; see [Images](#images) |
| `KIRI_PAGING` | When to page; see [Paging](#paging) |
| `NO_COLOR` | No colours, when set and not empty; see [no-color.org](https://no-color.org) |
| `XDG_CONFIG_HOME` | Where the configuration directory is; `~/.config` when unset |
