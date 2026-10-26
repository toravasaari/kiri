# Fixture fonts

Tests draw text in SVG images with these fonts instead of the system's, so
the results do not depend on what is installed. kiri itself uses the
system's fonts.

| File | Font | License |
|---|---|---|
| `DejaVuSans.ttf` | [DejaVu Sans](https://dejavu-fonts.github.io/) 2.37, unchanged; it covers Latin, Greek and Cyrillic | Bitstream Vera and Arev fonts licenses, see [`LICENSE`](LICENSE) |
| `DejaVuSans-Oblique.ttf` | DejaVu Sans Oblique 2.37, unchanged; the oblique face, so that tests can draw italic text, which the SVG renderer does not slant on its own | the same, see [`LICENSE`](LICENSE) |
