# Theme

kiri takes its colours and heading sizes from a theme: a file given with
`--theme`, `KIRI_THEME` or the [configuration file](config.md), a built-in
[paper theme](#paper-themes) chosen with `--light` or `--dark`, or else
`~/.config/kiri/theme.toml` (or `$XDG_CONFIG_HOME/kiri/theme.toml`) when it
exists. [Configuration](config.md#theme) gives the order. Every key is
optional; the ones you leave out keep the defaults below. A mistake in the
file is an error, not silently ignored.

## All headings at once

A `[headings]` table sets keys for every level; `[h1]` to `[h6]` then
change single levels:

```toml
[headings]
indent = 1
rule = false
underline = "single"

[h3]
underline = "dotted"
```

## Colours

A colour is one of:

- a name from the terminal's palette: `black`, `red`, `green`, `yellow`,
  `blue`, `magenta`, `cyan`, `white`, or the same with `bright-` in front;
- a palette number from `0` to `255`;
- `#rrggbb`;
- `none` for the terminal's default colour.

Palette colours follow your terminal's colour scheme, which is why the
defaults use them. With `NO_COLOR` set, kiri ignores every colour in the
theme.

## Defaults

```toml
# Maximum text width in cells; 0 means the full terminal width.
max_width = 100

# Cells kept free on both sides of paragraphs, lists, quotes, tables and
# code blocks. Headings start at the edge; rules span the whole terminal.
text_margin = 2

# A colour painted behind the whole document, or none to let the
# terminal's own (possibly transparent) background show.
background = "none"
# The colour of text that has no colour of its own, or none for the
# terminal's.
foreground = "none"
# The colour of text kiri plays down: image alt text, raw HTML and link
# targets written out. none dims it instead.
muted = "none"

link = "blue"
inline_code = "magenta"
# A colour, none, or code-block for the code block theme's background.
inline_code_background = "code-block"
# A space of background colour on each side of inline code.
inline_code_padding = true
# Inline code may wrap at its spaces; false moves it to the next line
# instead, unless it is longer than a line. In a table, false keeps it
# whole while the other columns keep room for their text.
inline_code_wrap = true
quote_bar = "bright-black"
rule = "bright-black"
list_marker = "bright-black"
table_border = "bright-black"

# A box of background colour around code blocks, one cell and one line
# wide. Without it, code starts at the margin and copies cleanly.
code_padding = true

# Code block colours, chosen by the terminal's background, or by --light
# and --dark.
# Available: base16-eighties.dark, base16-mocha.dark, base16-ocean.dark,
# base16-ocean.light, InspiredGitHub, Solarized (dark), Solarized (light).
code_theme_dark = "base16-ocean.dark"
code_theme_light = "InspiredGitHub"

# The language of a code block, named in its top right corner on the
# padding line above the code. It needs code_padding and colours.
show_language_label = true
# A colour, none, or code-block for halfway between the code block
# theme's text and background.
language_label = "code-block"

# scale: size as a multiple of the normal font (1-7). It needs kitty's
# text sizing protocol; other terminals show headings at normal size.
# rule: draw a line under the heading, across the whole terminal width.
# underline: none, single, double, curly, dotted or dashed. Styles other
#   than single need a kitty-compatible terminal.
# background: a colour filled across the whole width, or none.
# indent: in normal-size cells, whatever the heading's scale.
[h1]
scale = 3
color = "blue"
bold = true
italic = false
rule = false
underline = "single"
background = "none"
indent = 0

[h2]
scale = 2
color = "blue"
bold = true
italic = false
rule = false
underline = "single"
background = "none"
indent = 0

[h3]
scale = 2
color = "blue"
bold = false
italic = false
rule = false
underline = "dotted"
background = "none"
indent = 0

[h4]
scale = 1
color = "blue"
bold = true
italic = false
rule = false
underline = "single"
background = "none"
indent = 0

[h5]
scale = 1
color = "blue"
bold = false
italic = false
rule = false
underline = "dotted"
background = "none"
indent = 0

[h6]
scale = 1
color = "cyan"
bold = false
italic = true
rule = false
underline = "dotted"
background = "none"
indent = 0

# Alert blocks: quotes that start with [!NOTE], [!TIP], [!IMPORTANT],
# [!WARNING] or [!CAUTION].
[alerts]
# github: GitHub's colours for a dark or light background. terminal: the
# terminal's blue, green, magenta, yellow and red.
colors = "github"
# tint: a background mixed from the page and the kind's colour, where
# kiri knows both as RGB. none: only the bar and the title are coloured.
background = "tint"
# A colour for one kind, or auto for the one colors gives it.
note = "auto"
tip = "auto"
important = "auto"
warning = "auto"
caution = "auto"

# The pager's status line: a bar with a badge at its start. A colour, none,
# or auto. With every colour auto, the bar is the page mixed with
# bright-black, where kiri knows both as RGB, and the badge is in reverse
# video; where it does not, the bar is in reverse video and the badge in
# bold. An auto foreground is the page's.
[status_line]
background = "auto"
foreground = "auto"
badge_background = "auto"
badge_foreground = "auto"

# Search matches in the pager: every match shown, and the current one, the
# match the reader is at, in bold. A colour, none, or auto. With both
# colours of a kind auto, its matches are in reverse video; otherwise auto
# and none keep the text's own colour. Without colours, as with NO_COLOR,
# matches are in reverse video whatever the theme says.
[search]
match_foreground = "auto"
match_background = "auto"
current_foreground = "auto"
current_background = "auto"
# After each jump to a match, the current match fades in three quarters
# of a second from the text's own colours into the current match's; when
# the search goes around an end, the match number on the status line
# fades too, from the reverse of the bar's colours back into them. The
# fade needs both ends as RGB, from the theme or from the colours the
# terminal tells; without them, the current match is marked at once and
# the match number stands apart from the bar, bold and reversed, for half
# a second.
```

## Paper themes

`--light` and `--dark` choose a built-in theme that draws the document on a
page of fixed colours, as a browser does, whatever your terminal's colour
scheme. The page is in greys only: headings, links, quotes, rules, tables
and inline code differ in shade, weight and underline, not in colour. Code
blocks keep their colours, from a code theme that matches the page. Muted
text, such as image alt text, is a grey that keeps a contrast of at least
4.5:1 against the page, instead of the terminal's dim text.

The paper themes are the defaults above with these changes. To change one,
start a theme file of your own from its keys.

### Light paper

```toml
background = "#ffffff"
foreground = "#1f1f1f"
muted = "#6e6e6e"
link = "#000000"
inline_code = "#1f1f1f"
inline_code_background = "#ebebeb"
quote_bar = "#c8c8c8"
rule = "#c8c8c8"
list_marker = "#707070"
table_border = "#b4b4b4"
code_theme_dark = "base16-ocean.light"
code_theme_light = "base16-ocean.light"

[headings]
color = "#000000"

[h6]
color = "#555555"

[status_line]
background = "#e0e0e0"
foreground = "#1f1f1f"
badge_background = "#1f1f1f"
badge_foreground = "#ffffff"

[search]
match_foreground = "#000000"
match_background = "#cccccc"
current_foreground = "#ffffff"
current_background = "#1f1f1f"
```

### Dark paper

```toml
background = "#1e1e1e"
foreground = "#d4d4d4"
muted = "#8e8e8e"
link = "#ffffff"
inline_code = "#d4d4d4"
inline_code_background = "#353535"
quote_bar = "#505050"
rule = "#505050"
list_marker = "#9a9a9a"
table_border = "#5a5a5a"
code_theme_dark = "base16-eighties.dark"
code_theme_light = "base16-eighties.dark"

[headings]
color = "#ffffff"

[h6]
color = "#a0a0a0"

[status_line]
background = "#3a3a3a"
foreground = "#e0e0e0"
badge_background = "#d4d4d4"
badge_foreground = "#1e1e1e"

[search]
match_foreground = "#ffffff"
match_background = "#606060"
current_foreground = "#1e1e1e"
current_background = "#d4d4d4"
```

The [example themes](../examples/themes/README.md) try out variations of the
heading and code styles.
