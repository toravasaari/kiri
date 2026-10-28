# Fixture fonts

Tests draw text in SVG images, and lay out reading mode's text, with these
fonts instead of the system's, so the results do not depend on what is
installed. kiri itself uses the system's fonts.

| File | Font | License |
|---|---|---|
| `DejaVuSans.ttf` | [DejaVu Sans](https://dejavu-fonts.github.io/) 2.37, unchanged; it covers Latin, Greek and Cyrillic | Bitstream Vera and Arev fonts licenses, see [`LICENSE`](LICENSE) |
| `DejaVuSans-Oblique.ttf` | DejaVu Sans Oblique 2.37, unchanged; the oblique face, so that tests can draw italic text, which the SVG renderer does not slant on its own | the same, see [`LICENSE`](LICENSE) |
| `DejaVuSans-Bold.ttf` | DejaVu Sans Bold 2.37, unchanged; the bold face, for bold text in reading mode | the same, see [`LICENSE`](LICENSE) |
| `NotoSansMono.ttf` | [Noto Sans Mono](https://notofonts.github.io/) 2.014, the variable font `NotoSansMono[wght].ttf`, unchanged but renamed; inline code in reading mode | SIL Open Font License 1.1, see [`OFL.txt`](OFL.txt) |
| `NotoColorEmoji-subset.ttf` | [Noto Color Emoji](https://github.com/googlefonts/noto-emoji) 2.057 in its COLRv1 form, cut down to 😀, 🎉 and 🦀 and what they need with fontations' subsetter (klippa); colour emoji in reading mode | SIL Open Font License 1.1, Copyright 2022 Google Inc.; see [`OFL.txt`](OFL.txt) for the licence's text |
