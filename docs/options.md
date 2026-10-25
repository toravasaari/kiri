# Options

These are the options kiri takes on the command line.

| Option | Meaning |
|---|---|
| `--format FORMAT` | What the file is: `markdown`, `csv`, `tsv`, `mermaid`, `text` (shown as it is), `image`, or an image format such as `png` |
| `--mode auto\|plain\|ansi\|kitty` | Override what kiri detects |
| `--width N` | Render for `N` cells instead of the terminal width |
| `--theme FILE` | Colours and heading sizes; see [Theme](theme.md) |
| `--light`, `--dark` | A light or dark page in greys; see [Paper themes](theme.md#paper-themes) |
| `-M`, `--no-margins` | No margins or padding, so text and code copy cleanly |
| `-I`, `--no-images` | Show images as their alt text, without reading them, and diagrams as code |
| `--images` | Show images, whatever the environment and configuration say |
| `--max-image-size MB` | Largest image file to read: 10 MB unless set, at most 100 |
| `--paging always\|auto\|never` | When to show the document in the pager; `auto` unless set |
| `-p`, `-P` | The pager always (`--paging always`) or never (`--paging never`) |
| `--follow on\|off\|auto` | Follow a lone file in the pager: read it again whenever it changes, with the view kept where it is (`on`) or moved to a change (`auto`); pages the file unless paging is `never`; `off` unless set |
| `-f`, `--fa`, `-F` | Follow (`--follow on`), follow and move to a change (`--follow auto`) or not (`--follow off`) |
| `--follow-jump first\|last\|most` | Which change auto-follow moves the view to: the first, the last or the one that takes the most rows; `first` unless set |
| `--first-load wait\|background` | Whether the pager waits for the images before it shows the document, or shows the text at once and each image as it comes; only where the pager surely opens, as with `-p` or `-f`; `wait` unless set |
| `--front-matter table\|code\|hide\|off` | Front matter render style; `table` unless set |
| `--table-rows zebra\|lines\|indent\|underline\|off` | Table row style; `zebra` unless set |
| `--diagrams on\|off` | Diagrams in code blocks, such as Mermaid, drawn or shown as code; `on` unless set |
| `--render-errors inline\|stderr\|both` | Why a diagram was not drawn: below its code, in a warning, or both; `both` unless set |
| `--doctor` | Show what kiri finds out about the terminal and which settings it uses; see [The doctor report](doctor.md) |
| `--raw` | With `--doctor`: the report as Markdown text, for a bug report |
| `--list-emojis` | List the emoji shortcodes, such as `:smile:`: a table in a terminal, one line per emoji with tabs between the fields in a pipe or a file |
