# Theme

kiri reads its colours and heading sizes from
`~/.config/kiri/theme.toml` (or `$XDG_CONFIG_HOME/kiri/theme.toml`) when the
file exists, or from the file given with `--theme`. Every key is optional;
the ones you leave out keep the defaults below. A mistake in the file is an
error, not silently ignored.

## Colours

A colour is one of:

- a name from the terminal's palette: `black`, `red`, `green`, `yellow`,
  `blue`, `magenta`, `cyan`, `white`, or the same with `bright-` in front;
- a palette number from `0` to `255`;
- `#rrggbb`;
- `none` for the terminal's default colour.

Palette colours follow your terminal's colour scheme, which is why the
defaults use them.

## Defaults

```toml
# Maximum text width in cells; 0 means the full terminal width.
max_width = 100

link = "blue"
inline_code = "magenta"
# A colour, none, or code-block for the code block theme's background.
inline_code_background = "code-block"
# A space of background colour on each side of inline code.
inline_code_padding = true
quote_bar = "bright-black"
rule = "bright-black"
list_marker = "bright-black"
table_border = "bright-black"

# Code block colours, chosen by the terminal's background.
# Available: base16-eighties.dark, base16-mocha.dark, base16-ocean.dark,
# base16-ocean.light, InspiredGitHub, Solarized (dark), Solarized (light).
code_theme_dark = "base16-ocean.dark"
code_theme_light = "InspiredGitHub"

# scale: size as a multiple of the normal font (1-7). It needs kitty's
# text sizing protocol; other terminals show headings at normal size.
# rule: draw a line under the heading.
# underline: none, single, double, curly, dotted or dashed. Styles other
#   than single need a kitty-compatible terminal.
# background: a colour filled across the whole width, or none.
# indent: in characters of the heading's own size.
[h1]
scale = 3
color = "blue"
bold = true
rule = true
underline = "none"
background = "none"
indent = 0

[h2]
scale = 2
color = "blue"
bold = true
rule = true
underline = "none"
background = "none"
indent = 0

[h3]
scale = 1
color = "cyan"
bold = true
rule = false
underline = "none"
background = "none"
indent = 0

[h4]
scale = 1
color = "none"
bold = true
rule = false
underline = "none"
background = "none"
indent = 0

[h5]
scale = 1
color = "none"
bold = true
rule = false
underline = "none"
background = "none"
indent = 0

[h6]
scale = 1
color = "bright-black"
bold = true
rule = false
underline = "none"
background = "none"
indent = 0
```

The [example themes](../examples/themes/README.md) try out variations of the
heading and code styles.
