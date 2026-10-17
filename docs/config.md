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
# file; ~ is not expanded.
theme = "themes/mine.toml"

# false shows every image as its alt text, without reading it.
images = true
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

## Environment variables

| Variable | Meaning |
|---|---|
| `KIRI_CONFIG` | The configuration file to read instead of the default |
| `KIRI_THEME` | A theme file; see [Theme](#theme) |
| `KIRI_NO_IMAGES` | Images as alt text; see [Images](#images) |
| `NO_COLOR` | No colours, when set and not empty; see [no-color.org](https://no-color.org) |
| `XDG_CONFIG_HOME` | Where the configuration directory is; `~/.config` when unset |
