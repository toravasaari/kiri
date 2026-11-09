# The doctor report

`kiri --doctor` shows what kiri found out about the terminal, which
settings it uses and where each came from. This page tells how to read the
report when something looks wrong, such as images that show their alt
text or headings at normal size.

The report has these sections, in this order:

- **Problems**, only when there are any: a configuration or theme file
  that could not be read, or an environment variable with a value kiri
  does not know, which it leaves out.
- **Terminal**: what kiri found out about the terminal.
- **Settings**: the value of each setting and where it came from.
- **Files**: where kiri looks for its configuration file, its theme file
  and the work files of programs that draw diagrams, and whether they are
  there.
- **Image formats**: the image formats kiri reads.
- **Diagrams**: what draws each diagram language, and formulas (`math`).
- A sample of headings, a table and an image, in a terminal only. It shows
  how they look in this terminal; `--raw` leaves it out.
- **Probe answers**, only when the terminal was asked: what the terminal
  answered, byte for byte.

## Terminal

Most rows come from the probe: in `--mode auto`, kiri sends the terminal
a few questions and reads its answers. The [Probe answers](#probe-answers)
section below tells which answer leads to which row.

The **Terminal** row tells whether the probe ran:

| Terminal row | Meaning |
|---|---|
| A name, such as `kitty(0.47.1)` | The terminal answered and named itself |
| `unknown` | The terminal answered but did not name itself |
| `did not answer` | No answer within half a second; kiri uses colours and text styles only |
| `not asked: kiri runs in the background` | A program in the background must not read the terminal |
| `not asked in mode kitty`, or another mode | A `--mode` other than `auto` or `sixel` decides without asking |
| `none` | The output is not a terminal |

`TERM=` after it is the `TERM` environment variable, for reference; kiri
does not decide anything from it.

Inside tmux, tmux answers most questions itself, so the Terminal row says
`tmux 3.8` or so. When kiri wraps kitty's images for tmux, as
[Passthrough](config.md#passthrough) says, a **tmux** row follows: the
terminal outside tmux, as tmux names it, and the pane's
`allow-passthrough` and tmux's `focus-events`, such as
``passthrough for `kitty(0.47.1)`, allow-passthrough `on`, turned on by kiri, focus-events `on` ``.
With `allow-passthrough off: images cannot pass`, kiri could not turn the
option on; `tmux set -p allow-passthrough on` in the pane does it. With
`focus-events off`, tmux does not tell the pager when its window is shown
again, so images sent while it was hidden, as when following a file, stay
away until Ctrl+L. `set -g focus-events on` in tmux.conf turns it on;
kiri leaves it alone, as it is an option for every pane. tmux passes
links on only when its terminal features for the terminal outside include
`hyperlinks`; when they do not, the Hyperlinks row says so and names the
line for tmux.conf, such as
`set -as terminal-features ',xterm-kitty:hyperlinks'`, which kiri also
leaves to you. tmux gives a terminal its features when it attaches, so
after adding the line, reload tmux.conf (`tmux source-file` with its
path, such as `~/.tmux.conf`), detach and attach again; a new tmux
server is not needed. With `passthrough = "tmux"` kiri does not ask tmux
at all.

What each other row means, and where it comes from:

| Row | Meaning | From |
|---|---|---|
| Window | The window size in cells and pixels | The kernel's window size |
| Cell size | One cell in pixels, which sets the size of images. Sixel images need it | The window size; or else the terminal's answer; or else a default. Inside tmux the terminal's answer comes first |
| Graphics | Whether images can be drawn | The probe: an answer to the graphics query, and Unicode placeholders, below; or sixel in the device attributes. A terminal with sixel and kitty's protocol but no Unicode placeholders shows `sixel`, and, when sixel is drawn, the row adds that it also has kitty's protocol without placeholders |
| Sixel limits | The widest sixel in pixels and the number of colour registers; shown for sixel | The probe: the answers to the two sixel questions, else 1000 pixels and 256 colours |
| Graphics protocol | The image protocol and the terminal's name and version, as `kitty, from kitty(0.47.1)` or `sixel, from foot`; shown when there is graphics | The probe: the version answer |
| Image protocols found | Which image protocols the terminal answered to: `kitty`, `sixel`, both, or `none`. `not asked` when kiri asked nothing, as with `--mode kitty` or a pipe; `did not answer` when it asked and the terminal stayed silent | The probe: the graphics answer and the device attributes |
| Plays animations | Whether the terminal plays an animated image itself (kitty), or kiri redraws its frames in the pager (Ghostty), or only the first frame shows (Ghostty and others behind tmux) | The terminal's name and version; behind tmux, only kitty plays them |
| Text sizing | Whether headings can be drawn large | The probe: whether the cursor moved for the text sizing tests |
| Colours | 24-bit colour, fewer colours, or none | The probe; or else the `COLORTERM` environment variable |
| Background | Dark or light, which picks the code colours | The probe: the background colour |
| Hyperlinks | Whether links can be clicked | The probe; or else `TERM_PROGRAM` or `VTE_VERSION`; inside tmux, tmux's terminal features alone |
| Underline styles | Whether a curly or dotted underline can be drawn | The probe |

A terminal that answers the graphics query or draws the text sizing tests
speaks kitty's protocols. kiri then takes it to have 24-bit colour,
clickable links and underline styles too, without asking for them.

**Graphics** has six values. `yes, with Unicode placeholders` means
images are drawn. `yes, without Unicode placeholders: images show their
alt text` means the terminal answered the graphics query, but kiri cannot
tell whether it also has the Unicode placeholders through which kiri
places every image. No query tells that, so kiri trusts a terminal that
names itself kitty 0.28 or later or Ghostty 1.0 or later, or one that
also passed the text sizing tests, which only kitty from 0.40 on does.
Other terminals with placeholders get alt text for now. The fourth value,
`yes, sixel: every image row is drawn as a slice one cell high`, means the
terminal's device attributes list sixel, or `--mode sixel` asked for it, and
no placeholders were found; it adds that the terminal also has kitty's
graphics protocol when that answered. The fifth, `sixel, but the size of a
cell is unknown: images show their alt text`, is sixel when neither the
window size nor the terminal tells the size of a cell. The sixth, `sixel,
which kiri does not draw behind tmux: images show their alt text`, is sixel
found inside tmux.

## Settings

The **From** column tells where each value came from: a command-line
option, an environment variable, the configuration file, or the default.
When a value is not what you expect, this is the column to read: an
environment variable set in a shell start-up file overrides the
configuration file, for example. [Configuration](config.md) lists the
order.

**Heading sizes** lists h1 to h6 as times the normal size, from the
theme. A size kiri had to round to draw it shows both:
`1.3 (drawn 1.27 = 2 · 7/11)` is 7/11 of a font two rows high.

## Probe answers

The answers are shown as the terminal sent them, with the bytes that do
not print written out:

| Written | Byte |
|---|---|
| `\e` | ESC, which starts every answer |
| `\\` | A backslash; `\e\\` ends many answers |
| `\x60` | A backtick |
| `\x07` and other `\x..` | Any other byte, by its hex value |

The answers come in the order of the questions. A terminal leaves out the
answers to questions it does not know; only the last one, the device
attributes, comes from every terminal. The answers from kitty 0.47.1 on a
dark background start like this, with the colours shortened:

```text
\e[5;1R\e[5;3R\e[5;5R\e_Gi=31;OK\e\\\e]10;rgb:dddd/dddd/dddd\e\\\e]11;rgb:0000/0000/0000\e\\\e]4;0;rgb:0000/0000/0000\e\\ … \eP>|kitty(0.47.1)\e\\\e[?62;c
```

Each answer, what it looks like and what kiri takes from it:

| Answer | Looks like | What kiri takes from it |
|---|---|---|
| Cursor position, three times | `\e[5;1R` | The row and the column of the cursor before and after two text sizing tests, which draw a space two cells wide and a space at twice the size. When the columns grow by two each time, such as 1, 3 and 5, the terminal has text sizing: **Text sizing** is `yes`. The same column three times, such as 1, 1 and 1, means it has not |
| Graphics | `\e_Gi=31;OK\e\\` | The terminal speaks kitty's graphics protocol: **Graphics** is at least `yes, without Unicode placeholders`. No such answer means no images |
| Foreground colour | `\e]10;rgb:dddd/dddd/dddd\e\\` | The text colour, which kiri mixes with the background for shaded table rows and other muted colours |
| Background colour | `\e]11;rgb:0000/0000/0000\e\\` | The page colour. A dark one makes **Background** `dark`, a light one `light` |
| Palette, up to 16 | `\e]4;1;rgb:cccc/0000/0000\e\\` | The terminal's 16 colours, by number |
| Name and version | `\eP>\|kitty(0.47.1)\e\\` | The **Terminal** row. A kitty from 0.28 on, or a Ghostty from 1.0 on, has Unicode placeholders, so with the graphics answer **Graphics** is `yes, with Unicode placeholders` |
| Sixel limits | `\e[?2;0;1000;1000S`, `\e[?1;0;256S` | The widest sixel and the number of colour registers, asked with `\e[?2;4;0S` and `\e[?1;1;0S` in a second round that is sent when the device attributes list sixel, and always with `--mode sixel`, which forces sixel; otherwise a terminal without sixel is not asked. Without an answer kiri takes 1000 pixels and 256 registers |
| 24-bit colour | `\eP1$r0;38:2::1:2:3m\e\\` | kiri sets a colour in 24 bits and asks for the terminal's rendition (DECRQSS). A terminal that gives the colour back has 24-bit colour, as xterm does without setting `COLORTERM`: **Colours** is `24-bit` |
| Cell size | `\e[6;20;10t` | The height and the width of a cell in pixels. Asked when the window size has no pixels, as over some SSH connections and in containers, and always inside tmux, through tmux to the terminal outside: tmux gives a pane 16 × 32 pixels a cell when its own terminal does not tell the pixels |
| Device attributes | `\e[?62;4;c` | Attribute 4 means sixel: **Image protocols found** lists `sixel`, and **Graphics** is `sixel` when there is no kitty graphics answer. Behind tmux, both tmux's own answer and the wrapped answer from the outer terminal must list 4. Every terminal answers it, so it also marks the end of the answers. Inside tmux, tmux answers it at once, and kitty's answer to the same question sent through tmux comes after kitty's graphics answer and ends the answers there |

## When something looks wrong

**Images show their alt text.** Read the **Graphics** row. With `sixel`, the terminal has sixel, which kiri does not draw behind tmux or without the size of a cell. With `no`,
look for `\e_G` in the probe answers: without it, the terminal did not
answer the graphics query. Inside screen, the questions may not reach the
terminal at all; inside tmux, they reach it only when kiri wraps them and
the pane's `allow-passthrough` is on: read the **tmux** row. With
`without Unicode placeholders`, look at the name and version answer: kiri
draws images only in a terminal it knows has placeholders. Also check
that **Images** under Settings is `on`.

**Headings are at normal size.** Read **Text sizing**. With `no`, look at
the three cursor columns at the start of the answers. The same column
three times means the terminal ignored the tests, which happens in
terminals other than kitty and in kitty before 0.40.

**Code colours are hard to read.** Read **Background**. With `unknown`,
the terminal did not answer `\e]11;`, and kiri does not know whether the
page is light or dark; a paper theme (`--light` or `--dark`) decides. With the wrong
value, compare the `rgb:` value in the answer with the terminal's
background.

**Nothing is drawn, only plain text.** With `did not answer` or `none` in
the **Terminal** row, kiri fell back to colours and text styles, or to
plain text. `--mode kitty` forces all of kitty's features without asking,
to tell a detection problem from a drawing problem.
