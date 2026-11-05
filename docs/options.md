# Options

These are the options kiri takes on the command line.

| Option | Meaning |
|---|---|
| `--format FORMAT` | What the file is: `markdown`, `csv`, `tsv`, `mermaid`, `text` (shown as it is), `image`, or an image format such as `png` |
| `--mode auto\|plain\|ansi\|kitty` | Override what kiri detects; `auto` unless set; see [Mode](config.md#mode) |
| `--width N` | Render for `N` cells instead of the terminal width; 0 is the terminal's width; see [Width](config.md#width) |
| `--theme FILE-OR-NAME` | Colours and heading sizes, from a file or from a theme's name; see [Theme](theme.md) and [Theme](config.md#theme) |
| `--light`, `--dark` | Short for `--theme light` and `--theme dark`: a light or dark page in greys; see [Paper themes](theme.md#paper-themes) |
| `-M`, `--no-margins` | No margins or padding, so text and code copy cleanly |
| `--margins` | Keep the margins, whatever the environment and configuration say; see [Margins](config.md#margins) |
| `-I`, `--no-images` | Show images as their alt text, without reading them, and diagrams as code |
| `--images` | Show images, whatever the environment and configuration say |
| `--max-image-size MB` | Largest image file to read: 10 MB unless set, at most 100; see [Largest image](config.md#largest-image) |
| `--paging always\|auto\|never` | When to show the document in the pager; `auto` unless set |
| `-p`, `-P` | The pager always (`--paging always`) or never (`--paging never`) |
| `--follow on\|off\|auto` | Follow a lone file in the pager: read it again whenever it changes, with the view kept where it is (`on`) or moved to a change (`auto`); pages the file unless paging is `never`; `off` unless set |
| `-f`, `--fa`, `-F` | Follow (`--follow on`), follow and move to a change (`--follow auto`) or not (`--follow off`) |
| `--follow-jump first\|last\|most` | Which change auto-follow moves the view to: the first, the last or the one that takes the most rows; `first` unless set |
| `--initial-load wait\|background` | Whether the pager waits for the images before it shows the document, or shows the text at once and each image as it comes; only where the pager surely opens, as with `-p` or `-f`, or with paging on `auto` for a document longer than the window, each image counted at three rows; `wait` unless set |
| `--passthrough detect\|none\|tmux` | Whether kitty's images pass through tmux: wrapped inside tmux, with the pane's `allow-passthrough` turned on as `kitten icat` does (`detect`), always (`tmux`) or never (`none`); `detect` unless set |
| `--mouse`, `--no-mouse` | Whether the pager takes the mouse, so that the wheel scrolls it inside tmux too; on unless set; see [Mouse](config.md#mouse) |
| `--front-matter table\|code\|hide\|off` | Front matter render style; `table` unless set |
| `--reading`, `--no-reading` | Experimental: headings, paragraphs, lists and quotes drawn as images, in the system's fonts; on where the terminal draws images, unless turned off; `--no-reading` turns it off whatever the variable and the file say, and `--reading` asks for it by name, so that a terminal without images is warned of; see [Reading](config.md#reading) |
| `--wide wrap\|scroll` | Experimental: in reading mode, a code block or table wider than its area wraps to fit it, or scrolls sideways in the pager; `wrap` unless set; see [Wide](config.md#wide) |
| `--headings auto\|sized\|images\|plain` | How headings show: large with text sizing, as images, or in one row; `auto` unless set; see [Headings](config.md#headings) |
| `--table-rows zebra\|lines\|indent\|underline\|off` | Table row style; `zebra` unless set |
| `--diagrams on\|off` | Diagrams in code blocks, such as Mermaid, and formulas, drawn or shown as code; `on` unless set |
| `--diagram-timeout SECS` | How long a program may take to draw one diagram, in seconds; 20 unless set |
| `--diagram-panel on\|off` | Whether a diagram in the other page's colours lies on a panel; `on` unless set |
| `--render-errors inline\|stderr\|both` | Why a diagram was not drawn: below its code, in a warning, or both; `both` unless set |
| `--doctor` | Show what kiri finds out about the terminal and which settings it uses; see [The doctor report](doctor.md) |
| `--raw` | With `--doctor`: the report as Markdown text, for a bug report |
| `--list-emojis` | List the emoji shortcodes, such as `:smile:`: a table in a terminal, one line per emoji with tabs between the fields in a pipe or a file |
| `--completions SHELL` | Write the completion script for `bash`, `zsh`, `fish`, `elvish`, `powershell` or `nushell` to standard output; see [Completion](#completion) |

## Completion

`kiri --completions SHELL` writes a script that lets the shell complete kiri's options and their values, so `kiri --paging a` and Tab gives `always auto`. Options with a fixed list of values, such as `--paging`, complete those values; the other options complete file names.

For bash, put this in `~/.bashrc`:

```sh
eval "$(kiri --completions bash)"
```

For fish, put this in `~/.config/fish/config.fish`:

```fish
kiri --completions fish | source
```

For zsh, save the script as `_kiri` in a directory on your `fpath`, such as `~/.zfunc` (add `fpath+=~/.zfunc` before `compinit` in `~/.zshrc` if it is not there yet):

```sh
mkdir -p ~/.zfunc
kiri --completions zsh > ~/.zfunc/_kiri
```

`eval` and `source` run kiri at every shell start; a file does not, so it starts faster, but you write it again after a kiri update that adds options.
