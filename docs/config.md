# Configuration

kiri takes its settings from the command line, from environment variables
and from a configuration file. The command line is the strongest, then the
environment, then the file.

A setting that matters without the pager too, such as the width or
whether images show, can be given at all three: as a flag, as a variable
and as a key in the file. A setting about how the pager works, such as
`search_case`, is a key in the file only. The sections below give each
setting's flag, variable and key, and [Options](options.md) lists the
flags.

## Configuration file

kiri reads `$XDG_CONFIG_HOME/kiri/config.toml`, or
`~/.config/kiri/config.toml` when `XDG_CONFIG_HOME` is unset, empty or
not an absolute path. The
file is optional. `KIRI_CONFIG` names another file instead, and that file
must exist. A mistake in the file, an unknown key among them, is an error,
not silently ignored.

Every key is optional:

```toml
# Which terminal features to use: "auto", "plain", "ansi" or "kitty";
# see Mode below.
mode = "auto"

# Width in cells, instead of the terminal's; see Width below.
width = 100

# A theme file, or a theme's name: "dark", or "mine" for mine.toml in
# the themes directory; see Theme below. A relative path is relative to
# the directory of this file, and a leading ~/ is your home directory.
theme = "themes/mine.toml"

# false leaves out the margins, indents and padding, for copying text.
margins = true

# The largest image file to read, in whole megabytes, from 1 to 100.
max_image_size = 10

# false shows every image as its alt text, without reading it.
images = true

# When to show the document in the pager: "always", "auto" or "never".
paging = "auto"

# How a search in the pager treats case: "smart" or "literal".
search_case = "smart"

# Whether B in the pager paints the terminal's background one shade off;
# see Background offset below.
background_offset = true

# Whether the pager follows a lone file, reading it again whenever it
# changes: "on", "off" or "auto", which also moves the view to a change.
follow = "off"

# Which change auto-follow moves the view to: "first", "last" or "most".
follow_jump = "first"

# Whether the contents bar shows in the pager at the start: "auto", "on"
# or "off"; see Contents bar below.
toc = "auto"

# Whether moving the selection in the contents bar moves the document to
# the heading selected: true or false; see Contents bar below.
toc_preview = false

# Whether the pager waits for the images before it shows the document:
# "wait", or "background" to show the text at once.
initial_load = "wait"

# Whether kitty's images pass through tmux: "detect", "none" or "tmux";
# see Passthrough below.
passthrough = "detect"

# false leaves the mouse to the terminal in the pager; see Mouse below.
mouse = true

# false turns reading mode off, which draws the text as images on a
# terminal that can; see Reading below.
reading = true

# How many times an animation plays at most once you start it; 0 is
# without limit; see Animations below.
animation_plays = 3

# How wide code and tables show in reading mode: "wrap" or "scroll"; see
# Wide below.
wide = "wrap"

# How large reading mode shows text and pictures, from 0.5 to 2; see Zoom
# below.
zoom = 1

# Experimental: whether reading mode starts headings on whole terminal
# rows; see Snap to rows below.
snap_to_rows = false

# Front matter render style: "table", "code", "hide" or "off".
front_matter = "table"

# How headings show: "auto", "sized", "images" or "plain"; see Headings
# below.
headings = "auto"

# Table row style: "zebra", "lines", "indent", "underline" or "off".
table_rows = "zebra"

# Where kiri tells why a diagram could not be drawn: "inline", "stderr"
# or "both"; see Render errors below.
render_errors = "both"

# Diagrams in code blocks, such as Mermaid, and formulas; see Diagrams
# below.
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

## Mode

The first of these that is given decides which terminal features kiri
uses:

1. `--mode auto|plain|ansi|kitty|sixel`
2. `KIRI_MODE` with the same values; an empty value counts as unset, and
   any other value is left out with a warning
3. `mode` in the configuration file, such as `mode = "ansi"`
4. `auto`, which asks the terminal, and gives plain text in a pipe

`kitty` forces all of kitty's features without asking the terminal.
`sixel` draws images as sixel whether or not the terminal's device
attributes list it, which helps with a terminal that has sixel and does not
say so. It still asks for the size of a cell and for the limits of sixel,
and without a size for a cell images stay alt text. `auto` draws sixel in a
terminal whose device attributes list it, such as xterm started with
`-ti vt340`, unless the terminal has kitty's Unicode placeholders; behind
tmux it does not.

## Width

kiri renders for the width of the terminal. The first of these that is
given decides instead:

1. `--width 80`
2. `KIRI_WIDTH`, a whole number of cells: `KIRI_WIDTH=80 kiri doc.md`; an
   empty value counts as unset, and any other value is left out with a
   warning
3. `width` in the configuration file, such as `width = 80`
4. the terminal's width

A width of 0 means the terminal's own width, as `max_width = 0` does in a
theme. It is a choice like any other, so `width = 0` in the file beats
nothing but a flag or a variable, and `KIRI_WIDTH=0 kiri doc.md` uses the
terminal's width whatever the file says.

## Margins

The theme's margins, heading indents and code block padding are kept
unless one of these, the first that is given, leaves them out:

1. `--no-margins` or `-M` leaves them out, `--margins` keeps them; the two
   cannot be given together
2. `KIRI_MARGINS`: `on` or `off`, `1` or `0`; an empty value counts as
   unset, and any other value is left out with a warning
3. `margins` in the configuration file, such as `margins = false`
4. on

## Largest image

kiri reads image files up to 10 MB. The first of these that is given
decides the limit, in whole megabytes from 1 to 100; larger images show
their alt text:

1. `--max-image-size 20`
2. `KIRI_MAX_IMAGE_SIZE`, such as `KIRI_MAX_IMAGE_SIZE=20`; an empty
   value counts as unset, and any other value is left out with a warning
3. `max_image_size` in the configuration file, such as
   `max_image_size = 20`
4. 10

## Theme

The first of these that is given chooses the [theme](theme.md):

1. `--theme FILE-OR-NAME`; `--light` and `--dark` are short for `--theme
   light` and `--theme dark`, the [paper themes](theme.md#paper-themes)
2. `KIRI_THEME`, when it is not empty
3. `theme` in the configuration file
4. `theme.toml` in the configuration directory
   (`$XDG_CONFIG_HOME/kiri/theme.toml` or `~/.config/kiri/theme.toml`),
   when it exists
5. the built-in theme

`--theme`, `KIRI_THEME` and `theme` take a path or a name. A value with no
path separator and no `.toml` ending is a name: `mine` is `mine.toml` in
the themes directory (`$XDG_CONFIG_HOME/kiri/themes/` or
`~/.config/kiri/themes/`), and `light` and `dark` are the built-in paper
themes unless a `light.toml` or `dark.toml` is in that directory. Anything else is a path, as
`~/themes/mine.toml` or `./mine`. An unknown name is an error that lists
the names kiri knows, and says how to write a file:

```sh
kiri --theme mine doc.md        # ~/.config/kiri/themes/mine.toml
KIRI_THEME=dark kiri doc.md     # the dark paper theme
kiri --theme mien doc.md
# kiri: theme `mien` is not one of the themes: dark, light, mine
# (for a file in this directory, write ./mien)
```

A name in the configuration file is not relative to the file; a path is,
as before: `theme = "dark"` against `theme = "themes/mine.toml"`.

A file named by `--theme`, `KIRI_THEME` or the configuration file must
exist. `--light`, `--dark` and `--theme` cannot be given together. The
paper themes choose the code colours for a light or a dark background
instead of the terminal's background.

## Images

Images are shown by default. The first of these that is given decides
instead:

1. `--images` turns them on, `-I` or `--no-images` off; the two cannot be
   given together
2. `KIRI_NO_IMAGES`, a switch like `KIRI_MOUSE`, but for the thing turned
   off: `1` (or `on`) turns images off, and `0` (or `off`) turns them on
   whatever the configuration file says; an empty value counts as unset,
   and any other value, such as `yes`, is left out with a warning:
   `KIRI_NO_IMAGES=yes kiri doc.md` says
   ``KIRI_NO_IMAGES: `yes` is not on, off, 1 or 0; left out``
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

`auto` follows one rule for everything kiri shows: when what the run would
print is longer than the window, the pager opens, and a shorter output is
printed. It does not matter what the files are: Markdown, code or text,
CSV, images, diagrams, or any mix of them, whose rows count together, each
file with its header. So `kiri *.png` opens the pager when the images
together take more than a screen, and prints them when they fit; one image
on its own is scaled to fit the window, so it is printed. A lone document that
is [followed](#follow) is paged however short.

A raster image file whose picture cannot be read counts as a picture of
the size its header gives, so the pager may open for a list of broken
images, although the error messages that stand in for them would fit the
window. A broken SVG has no header size, and is shown as an error.

Two things are paged only with `always`: a document from standard input,
often another command's output, and the `--doctor` report. `never` pages
nothing. Whatever the setting, kiri prints the document when its output is
not a terminal or `--mode plain` asks for text without escape sequences.

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
A reload of a followed file keeps the widths that `r` or `:w` fitted the
columns of a long table to, so the view stays calm: a wider cell that comes
wraps in its column.
With `auto`, a reload that changed the document moves the view to a
change, which [Follow jump](#follow-jump) chooses. A change is a run of
blocks that changed or were added, one after another; blocks removed with
neither beside them are a change where they were. The view stays when it
shows the start of the change, or is inside it already, as in a long
change you scrolled into. Otherwise the start goes near the top, below
three rows of what comes before it (fewer in a window of fewer than
twelve rows), or the last page shows it. A change in a list, a quote or
an alert moves the view to the start of that list, quote or alert. `r`
moves to a change too while following is `auto`.

`F` in the pager goes from `off` to `on` to `auto` and back to `off`, and
the status line says `F follow` after the position while following is on
and `F auto` while it is auto. Following notices a change by the file's
modification time and length, so on a filesystem that keeps the time only
to the second, an edit within the same second that keeps the length waits
for the next change. A document from standard input, or several files,
cannot be followed yet: with `-f`, `--fa`, `--follow on` or `--follow
auto` the pager says `nothing to follow`, and with `KIRI_FOLLOW` or
`follow` in the configuration file it says nothing.

## Follow jump

When a reload changed the document in more than one place, auto-follow
moves the view to one of the changes, which the first of these that is
given chooses:

1. `--follow-jump first|last|most`
2. `KIRI_FOLLOW_JUMP`: `first`, `last` or `most`; an empty value counts
   as unset, and any other value is left out with a warning
3. `follow_jump` in the configuration file
4. `first`

`first` is the change nearest the start of the document, `last` the one
nearest its end, and `most` the one that takes the most rows on the
screen, so a new image counts by its height; of two that take as many,
the first.

After the jump, the lines of the change the view went to get a lighter
background, so that the reader sees why the view moved: the page colour
mixed 15 % toward the text colour, across the width of the text. It stays
for 1 second and then fades out over 2 seconds. This happens whether or
not the view had to move, for `r` as well, and only while following is
`auto`; a removal has no lines to show, so it gets no highlight. A line
that has a background of its own, such as a code block's, gets the same
mix over it. Scrolling does not move the highlight away from its lines,
nor does an image that comes after the reload, and a new reload replaces
it. On a terminal where the page colours are
not known as RGB, or without 24-bit colour, there is no highlight.

## Initial load

When the pager surely opens, the first of these that is given decides
whether it waits for the document's images and diagrams before it shows
the document, or shows the text at once and each image as it comes:

1. `--initial-load wait|background`
2. `KIRI_INITIAL_LOAD`: `wait` or `background`; an empty value counts as
   unset, and any other value is left out with a warning
3. `initial_load` in the configuration file
4. `wait`

With `background`, each image and diagram shows as it does without its
image until it comes, a diagram as its code and an image as its alt
text, with a thin moving band below it, and the status line says
`[rendering: 1/3]` in place of the name meanwhile, as after a
[reload](#follow). The view keeps its top line as the images come, so
the text below an image moves down when it arrives; that is why `wait`
is the default. The warnings about images and diagrams that failed, which
`wait` writes on standard error before the pager opens, come once the
pager has closed, for what still fails then.

This holds where the pager surely opens: when paging is `always`, as with
`-p`, or when it follows a lone document, as with `-f`, and with paging on
`auto` when the document is longer than the window with each image and
diagram that has not loaded counted at three rows. A shorter document
waits for its images, and is then paged or printed as their real height
says. So, with `wait`, a document that shows the hint below can still
be printed once its images turn out smaller than three rows each. A
printed document always waits: in a window of 40 rows,
`kiri --initial-load background notes.md` shows a document of 50 rows of
text at once, and waits for one of 30 rows of text and two diagrams.
With several files, the images of the documents among them load in the
background too, and an image or diagram file waits for its image, among
others or on its own.

With `wait`, in those same places Space or Enter cuts the wait short:
while standard error says `kiri: loading images 1/3… (space: open
now)`, either key opens the pager at once, with the images that have
come, and the rest load in the background as with `background`. Other
keys do nothing meanwhile and are not kept for the pager, and Ctrl-C
ends kiri as it does whenever it waits. Image files named on the command
line are read before the pager opens, and a key does not skip that.
Where the pager may not open,
such as in print mode or with paging on `auto` for a document that
fits, there is no hint and the keys do nothing, and so it is when
standard error is not a terminal, as in `kiri -p notes.md 2>log`, where
the line cannot show.

## Passthrough

Inside tmux, kitty's images reach kitty only when kiri wraps them in
tmux's passthrough. The first of these that is given decides whether it
does:

1. `--passthrough detect|none|tmux`
2. `KIRI_PASSTHROUGH`: `detect`, `none` or `tmux`; an empty value counts
   as unset, and any other value is left out with a warning
3. `passthrough` in the configuration file
4. `detect`

With `detect`, kiri wraps the images when it runs inside tmux, that is
when `TMUX` is set and not empty, and turns on the pane's
`allow-passthrough` when it is `off`, as `kitten icat` does; a pane
with `on` or `all` is left as it is. With `tmux`, kiri wraps them
always and runs no tmux command, and with `none` it never wraps them.
Headings stay one row high inside tmux, as tmux cannot pass kitty's
text sizing, and an image that loads in the background has no moving
band below it; the status line still says `[rendering: 1/3]`.

tmux passes images only while kiri's pane is shown, so the pager sends
the images it drew meanwhile again when the pane is shown again. tmux
tells it so only with
`set -g focus-events on` in tmux.conf, which kiri does not change, as
it is an option for every pane; without it, Ctrl+L sends them again.
tmux also drops output when kitty cannot keep up with it, so after
printing a document, and in the pager after drawing images, kiri asks
kitty whether it holds them and sends again those it does not. When
kitty does not answer a printed document's check, as when the pane is
not shown, kiri waits half a second and goes on, and it asks no more
for the other files it prints, so that `kiri *.md` does not wait for
each of them.

## Mouse

The pager takes the mouse, so that the wheel scrolls it three lines a
notch, inside tmux too. The first of these that is given decides
instead:

1. `--mouse` turns it on, `--no-mouse` off; the two cannot be given
   together
2. `KIRI_MOUSE`: `on` or `off`, `1` or `0`; an empty value counts as
   unset, and any other value is left out with a warning
3. `mouse` in the configuration file, `true` or `false`
4. on

While the pager has the mouse, a click on a link follows it at once, as
a browser does and as Enter does on a selected link: a link to a place in
the document jumps there (Backspace comes back), a link to a Markdown file
opens it, and any other address is copied. A click on a picture that is in
a link follows the link. A click on a picture that plays starts it, and
a click on text with no link does nothing. Kitty selects text when you
hold Shift as you drag, and a click with Shift is kitty's, not the
pager's. With more than one view open, the status line tells the path
of their file names, without directories (`start.md › other.md`), and a click
on a name before the last goes back to that view. The path takes at most
half of the status line, whatever link or picture is selected, so it does not
change as Tab moves between them. When it does not fit in that, the start of
the path is cut to `…`, and a click on it goes back to the nearest view it
stands for. The mouse's back button goes back, as Backspace and Alt+Left
do, and its forward button goes forward, as Alt+Right does: to the file
that going back left, at the place it was left. A new link that opens a
view forgets what was to go forward to. Alt+Left and Alt+Right need a
terminal that sends Alt with the arrow as a modified key (`CSI 1;3D`), as
kitty, Ghostty, xterm and tmux do; one that sends Alt as an Escape prefix does
not get them. With the help open, the wheel
scrolls it, also outside its frame, and the document under it stays where it
is. When the help has more rows than show, a bar by its right edge tells where
you are: a click on the track above or below the bar moves a page that way,
and you can drag the bar. A click elsewhere in the frame does nothing, and a
click outside it closes the help and does nothing else, so a link under the
click is not followed. For the drag the pager asks the terminal for the
motion of the mouse while a button is held, and not otherwise. With the mouse
off, kitty turns
the wheel into arrow keys in the pager, and inside tmux with `mouse on`
the wheel does not scroll the pager: tmux 3.6 drops it, and older tmux
may go into its copy mode. A printed document leaves the mouse alone
either way.

## Contents bar

In the pager, a bar at the left lists the headings of the document, with
the one you are reading marked. The first of these that is given decides
whether the bar shows when the pager opens:

1. `--toc auto|on|off`
2. `KIRI_TOC`: `auto`, `on` or `off`; an empty value counts as unset, and
   any other value is left out with a warning
3. `toc` in the configuration file
4. `auto`

With `auto`, the bar shows when the document keeps 80 columns beside it,
which a window of 110 columns or more does. With `on`, it shows when the
document keeps 40 columns, so a narrower window shows it too. With `off`,
it stays hidden until `T` or `t` shows it, in a window of any width:

```sh
kiri --toc off README.md
```

`T` shows and hides the bar for the rest of the run, whatever the setting
says at the start. The setting is only about the pager. When the document
is printed rather than shown in the pager, no contents are printed, whatever
`toc` says.

With the focus in the bar (`t`), moving the selection can move the document
too, if the configuration file says `toc_preview = true`; it is off unless
set, and there is no flag or variable for it. The document then shows the
heading selected, and Esc, `0` or `T` puts it back where it was when the
focus came to the bar. Enter keeps the place the document has, and Backspace
goes back to where it was before the visit:

```toml
toc_preview = true
```

## Reading

Reading mode is experimental. It draws headings, paragraphs, lists and
quotes as images, in the system's fonts, so the text looks as a browser
shows it, and it is on unless you turn it off. It needs a terminal that
draws images, such as kitty; elsewhere the document is shown as text, and
nothing is said about it, since you did not ask for reading mode. With
`--reading`, `KIRI_READING=1` or `reading = true` in the configuration
file you did ask, and kiri then warns that it is showing the text. The
text cannot be selected or copied, as it is drawn as images. For that
reason `kiri --doctor` and `kiri --list-emojis` show as text unless you
asked for reading mode by name; `M` in the pager turns it on for them. The
same goes for a CSV or TSV file, or any file shown with `--format text`,
when it is the only kind of file you give: `kiri data.csv` shows the table
as text. Next to a Markdown file they share its reading mode, as it is one
choice for the whole run.

The first of these that is given decides whether it is on:

1. `--reading` turns it on, `--no-reading` turns it off; they cannot be
   given together
2. `KIRI_READING`, `1` or `on` to turn it on, `0` or `off` to turn it off
3. `reading` in the configuration file, `true` or `false`
4. on

A configuration file with `reading = false` shows every document as text,
and `--reading` turns reading mode on again for one run:

```sh
kiri --reading notes.md   # reading mode, whatever the file says
```

## Animations

An animated GIF, WebP or APNG shows its first frame in the pager, with a
play button in the middle (a picture under 48 pixels on a side has none), in
a document (`![](demo.gif)`) and on its own (`kiri demo.gif`), and plays when
you start it: a click on it, or `p` or Enter on the one Tab has selected.
Between the start and the first frame of the play the frames are built, which
takes a moment for a large picture; the picture shows a turning half circle
in a dark disc then, turning eight steps a second, for half a second at least
(the frames are built in a few milliseconds, which is too quick to see), and
plays when the frames are ready.
One animation plays at a time, so starting one stops the one that plays or
waits, and a stopped animation is back on its first frame. A picture in a link
follows the link on a click and on Enter, and `p` starts it.

An animation plays as many times as its file says, but at most
`animation_plays` times, 3 unless the configuration file says otherwise; 0
is without limit. So a GIF that loops without end plays 3 times and then
rests on its last frame, and one that plays twice plays twice. An animation
that has played its plays stays on its last frame, with a replay icon in its
bottom right corner, until you read the file again with `r`; a click anywhere
on it plays it again.

The status line tells the animation that plays, or played last, in a pill
at its right edge: a mark (▶ playing, ■ stopped or ended), the picture's alt
text (or its file's name), a bar of one play and the time, `3.2 / 8.0 s` for
an animation under a minute and `1:03 / 2:10` from a minute on. It appears
with the first start, stays when its picture is off the screen, and is drawn
four times a second while the animation plays, its time moving in
quarter-second steps. The animation or link Tab selected is told after the
file's name, an animation in the yellow of its frame and a link in blue, whole
when the line has room and cut with `…` when it has not; it is told while its
frame is shown, also for the animation in the pill. While the pill is on the
line the key hints (`? help`, `q quit`) are not shown; they return when the
pill goes. When the window narrows, the file's name goes first, then the
pill's time, then half its bar, then the words and the position, the
selected animation or link, and last the pill. The search stays, with the
position it goes with, and a selected link or animation stays longer than the
pill's time; with a search, a long selected piece is cut to the room that is
left beside the search and the pill.

Tab selects the first animation or link on the screen, and goes on to the next
in the whole document, round at the end; Shift+Tab goes back. The view
scrolls to show the selected piece whole. A piece cut at an edge of the window
moves as little as it takes: a picture that is partly below the window comes up
until its bottom edge is at the bottom, and one partly above goes down until
its top edge is at the top. A piece that is wholly out of the window is shown
in the middle of it, as near as the ends of the document allow. A picture
taller than the window has its top at the top of it, unless the window's top
is already inside the picture: then the view stays. A piece that is wholly in
the window does not move it, and a click on a picture does not scroll.

The status line tells a selected animation in the yellow of its frame and a
selected link in the blue of its own. Esc lets the selection go, and a second
Esc takes the pill off the status line. A click on the status line scrolls to
the picture the pill tells of and selects it (any click on the line does,
whatever its column, except one on a name of the path of views or on its `…`).
`p` with nothing selected starts the first animation on the screen, or when
there is none, scrolls to the pill's picture and starts it.

Scrolling, resizing, searching and opening the help or the prompt stop the
animation that plays. Printing shows the first frame, since kiri is gone
when an animation plays.

## Wide

In reading mode, a code block or table wider than the area it is drawn in
either wraps to fit it, or keeps its lines and words whole and scrolls
sideways in the pager. The first of these that is given decides:

1. `--wide wrap|scroll`
2. `KIRI_WIDE` with the same values
3. `wide` in the configuration file
4. `wrap`

Printing always wraps, since a printed page cannot scroll. In the pager,
`W` switches between the two. A configuration file with both of these
shows a wide code block as a band that scrolls sideways:

```toml
reading = true
wide = "scroll"
```

## Zoom

In reading mode, `zoom` sets how large the text and the pictures are, as a
browser's zoom does: one factor for both, so a page keeps its proportions.
The factor is a number from 0.5 to 2, such as `1.25`, and it is rounded to
a whole percent. The first of these that is given decides:

1. `--zoom FACTOR`
2. `KIRI_ZOOM` with the same values
3. `zoom` in the configuration file, a number such as `1.25` or `2`
4. `1`

The column of text is 42 times the text's size wide, so it widens with the
zoom until the window is full, and then wide content scrolls or wraps as
[Wide](#wide) says. Printing zooms too. The text layout does not zoom: it
follows the terminal's font, which is changed in the terminal.

In the pager, `z` makes the text and the pictures one step larger and `Z`
one step smaller: 50, 67, 80, 90, 100, 110, 125, 150, 175 and 200 %. A zoom
between two steps moves to the next step in the direction pressed. The
change lasts until kiri exits and is not saved; to keep a zoom, set
`zoom`. The key works in text mode too, and the zoom is there when `M`
turns reading mode on.

Blocks that reading mode lays out as cells do not zoom: an image file shown
alone, an error and the header of a file. They stay as the text layout draws
them, in the column of the zoomed text.

```toml
reading = true
zoom = 1.25
```

## Snap to rows

Experimental. In reading mode, `snap_to_rows = true` starts the headings
of levels 1 to 4 on whole terminal rows, so that a jump to a heading shows
it from the top row of the screen, with nothing of the paragraph above.
The gap above a heading then grows or shrinks by less than a row, and it
is never smaller than the gap between two paragraphs. Printing leaves the
headings where they are, since nothing jumps to them there. It is off
unless set, and it may become a lasting setting later.

`KIRI_SNAP_TO_ROWS`, `1` or `on` and `0` or `off`, overrides the
configuration file, and `:s` in the pager turns it on or off for the run.

## Search case

A search in the pager ignores case when its term has no upper-case letter,
and matches case exactly when it has one: `kiri` finds `Kiri` too, but
`Kiri` finds only `Kiri`. `search_case = "literal"` in the configuration
file makes every search match case exactly. Only the configuration file
sets this; there is no flag or environment variable for it.

## Background offset

`B` in the pager paints the page in the terminal's background colour when
the theme has none. kitty shows its own background, which may be
translucent or an image, through a cell painted in exactly that colour,
so kiri paints it one shade off, such as `#010101` for black, which looks
the same. `background_offset = false` in the configuration file paints
the exact colour. Only the configuration file sets this.

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
A block with nothing in it, such as `---` and then `---`, shows nothing in
`table` and `code`.
YAML, TOML or JSON that does not parse shows as a code block in the
table style, with a warning naming the line of the file, such as
`kiri: front matter, line 4: …`. CSV and TSV files have no front matter.

In the pager, a reload whose document has such a warning says `reloaded` as
any reload does, and shows the warning twice, as below. A document that has
the warning when it is first opened gets the same, after kiri has printed the
warning on standard error as it always does.

- The status line gets `⚠ 1` before the position, the number being how many
  warnings the document shown has. The mark stays until a reload brings a
  document without warnings, and its number flashes when it changes, as the
  match number does when a search goes around an end. In a narrow window
  the mark is left out right after the word that tells kiri follows the
  file.
- The row just above the status line gets a line of text at the right edge,
  over whatever the document has there, in the theme's `error` colour on the
  page colour: `⚠ [:3] front matter: …`, where `[:3]` is the line of the file
  the parser stopped at, left out when it does not tell. With more than one
  warning the first is shown, followed by `and 2 more warnings…` in
  italics and a dimmer colour. A line too long for the window is cut with
  `…`. It stays for 5 seconds and then fades out over 0.75 seconds into the
  page colour, or goes at once when the colours are not known as RGB. A key
  does not remove it; a new reload replaces it, or removes it when the new
  document has no warnings. A window with no row for it above the status
  line does not show it.

## Headings

The first of these that is given decides how the headings of the document
show, those not inside a quote, a list or an alert, which are always one
row:

1. `--headings auto|sized|images|plain`
2. `KIRI_HEADINGS` with the same values, as for `KIRI_FRONT_MATTER`
3. `headings` in the configuration file
4. `auto`

`auto` draws headings large with kitty's text sizing, or, in a terminal
without it that draws images, such as kitty inside tmux, as images of their
text at the same sizes. `sized` draws them large only with text sizing.
`images` draws them as images wherever images are drawn, even with text
sizing, and otherwise as `auto` does. `plain` keeps every heading in one
row, bold and in its colour. Where a heading is not drawn large, it is one
row as with `plain`; `--no-images` also turns heading images off.

A heading too long for the window wraps at its spaces into several lines,
drawn as an image or with text sizing alike. Only a single word wider than
the window, with any text right against it such as a comma after inline
code, makes a heading image smaller, to fit on one line.

In a heading drawn as an image, inline code and keys (`<kbd>`) look as
they do in a sized heading: in a monospace font, in the colour of inline
code, on its background, and emphasis is in italics. Links, strikethrough,
`<mark>` and other inline styles are drawn as the heading's own text, and a
link in a heading image cannot be clicked. The image's font is the
system's sans-serif one, not the terminal's, and a colour of the
terminal's palette that the terminal does not tell is drawn as xterm's
default for it. The theme's `inline_code_wrap` does not apply there: a
heading image wraps at every space, also inside inline code, and text
right against inline code or emphasis, such as a comma after it, stays on
its line.

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

Which diagrams kiri draws, and with what, is in [Diagrams](diagrams.md),
and how it draws formulas in [Formulas](formulas.md).

Two more settings of the `[diagrams]` table can also be given as a flag
and a variable, the flag first:

| Key | Flag | Variable | Example |
|---|---|---|---|
| `timeout` | `--diagram-timeout SECS` | `KIRI_DIAGRAM_TIMEOUT` | `KIRI_DIAGRAM_TIMEOUT=60 kiri doc.md` |
| `background_panel` | `--diagram-panel on\|off` | `KIRI_DIAGRAM_BACKGROUND_PANEL` | `kiri --diagram-panel off doc.md` |

The flags take the names, `on` or `off` for the panel; the variables
also take `1` and `0`. An empty variable counts as unset, and any other
value is left out with a warning. `diagrams.mermaid.command` and
`auto_detect` are tables per language and stay in the file.

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
What failed while the pager showed the document, loading in the
background after a reload or with the [initial load](#initial-load) in
the background, is told on standard error once the pager has closed: the
images and diagrams that still fail in the document as it is then, each
once.

## Environment variables

| Variable | Meaning |
|---|---|
| `KIRI_CONFIG` | The configuration file to read instead of the default |
| `KIRI_THEME` | A theme file; see [Theme](#theme) |
| `KIRI_NO_IMAGES` | `1` turns images off, `0` on; see [Images](#images) |
| `KIRI_PAGING` | When to page; see [Paging](#paging) |
| `KIRI_FOLLOW` | Whether the pager follows a file; see [Follow](#follow) |
| `KIRI_FOLLOW_JUMP` | Which change auto-follow moves the view to; see [Follow jump](#follow-jump) |
| `KIRI_TOC` | Whether the contents bar shows when the pager opens; see [Contents bar](#contents-bar) |
| `KIRI_INITIAL_LOAD` | Whether the pager waits for the images; see [Initial load](#initial-load) |
| `KIRI_PASSTHROUGH` | Whether kitty's images pass through tmux; see [Passthrough](#passthrough) |
| `KIRI_MOUSE` | Whether the pager takes the mouse; see [Mouse](#mouse) |
| `KIRI_READING` | Whether reading mode is on; see [Reading](#reading) |
| `KIRI_WIDE` | How wide code and tables show in reading mode: `wrap` or `scroll`; see [Wide](#wide) |
| `KIRI_ZOOM` | How large reading mode shows text and pictures, from 0.5 to 2; see [Zoom](#zoom) |
| `KIRI_SNAP_TO_ROWS` | Whether reading mode starts headings on whole rows; see [Snap to rows](#snap-to-rows) |
| `KIRI_MODE` | Which terminal features to use; see [Mode](#mode) |
| `KIRI_WIDTH` | Width in cells; see [Width](#width) |
| `KIRI_MARGINS` | Whether the theme keeps its margins; see [Margins](#margins) |
| `KIRI_MAX_IMAGE_SIZE` | The largest image file to read, in MB; see [Largest image](#largest-image) |
| `KIRI_DIAGRAM_TIMEOUT` | How long a program may take to draw one diagram; see [Diagrams](#diagrams) |
| `KIRI_DIAGRAM_BACKGROUND_PANEL` | Whether a diagram in the other page's colours lies on a panel; see [Diagrams](#diagrams) |
| `KIRI_FRONT_MATTER` | Front matter render style; see [Front matter](#front-matter) |
| `KIRI_HEADINGS` | How headings show; see [Headings](#headings) |
| `KIRI_TABLE_ROWS` | Table row style; see [Table rows](#table-rows) |
| `KIRI_DIAGRAMS` | Diagrams drawn or shown as code; see [Diagrams](diagrams.md) |
| `KIRI_RENDER_ERRORS` | Where kiri tells why a diagram was not drawn; see [Render errors](#render-errors) |
| `NO_COLOR` | No colours, when set and not empty; see [no-color.org](https://no-color.org) |
| `HOME` | Your home directory, `~`, for the default configuration and cache directories, a theme under `~/` and programs found under it; ignored when empty or not an absolute path |
| `XDG_CONFIG_HOME` | Where the configuration directory is; `~/.config` when unset or not an absolute path |
| `XDG_CACHE_HOME` | Where kiri's cache directory is, for the files of [diagram commands](diagrams.md#diagram-commands); `~/.cache` when unset or not an absolute path |
| `TMUX` | Set and not empty inside tmux; see [Passthrough](#passthrough) |
| `TMUX_PANE` | The tmux pane whose `allow-passthrough` kiri turns on; see [Passthrough](#passthrough) |
| `TMPDIR` | The temporary directory, for the files of [diagram commands](diagrams.md#diagram-commands) when the cache directory cannot be used; `/tmp` when unset or empty |
