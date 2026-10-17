# Visual check: images

A document for checking kiri's images by eye in a real kitty window, beside
[`visual-check.md`](visual-check.md), which shows one image of each format.
Each section says what you should see; anything else is a bug.

Images show in kitty. Elsewhere, and when an image cannot be loaded, its alt
text appears in brackets. An image is as large next to the text as in a
browser, never enlarged, and never wider than the text. kiri warns on stderr
about the images it cannot load: here the five in the last section.

View it with the default theme, on a document background, and without
colours:

```sh
cargo run -- examples/visual-check-images.md
cargo run -- --theme examples/themes/opaque.toml examples/visual-check-images.md
NO_COLOR=1 cargo run -- examples/visual-check-images.md
```

Try a wide window and a narrow one (about 40 columns), where the wide images
shrink to the narrower text and the small ones keep their size.

## Scaling

Red, green, blue and yellow quadrants on a black grid, about 32 columns
wide and 12 rows high:

![Test pattern](../tests/fixtures/images/pattern.png)

A checkered icon of about two cells, not enlarged:

![Icon](../tests/fixtures/images/icon.png)

A gradient from blue to yellow, as wide as the text:

![Panorama](../tests/fixtures/images/panorama.png)

A tall gradient from yellow to blue, about 20 columns wide:

![Portrait](../tests/fixtures/images/portrait.png)

A lake in portrait format, and a large photo of a fern, 4845 pixels wide,
both shrunk to the text width:

![Portrait photo](../tests/fixtures/images/photo-portrait.jpg)

![Fern](../tests/fixtures/images/photo-large.jpg)

## EXIF orientation

The same photo of a lake under clouds eight times, about 60 columns wide,
each upright although the files store it turned and mirrored in every way
EXIF allows:

![Orientation 1](../tests/fixtures/images/exif-1.jpg)

![Orientation 2](../tests/fixtures/images/exif-2.jpg)

![Orientation 3](../tests/fixtures/images/exif-3.jpg)

![Orientation 4](../tests/fixtures/images/exif-4.jpg)

![Orientation 5](../tests/fixtures/images/exif-5.jpg)

![Orientation 6](../tests/fixtures/images/exif-6.jpg)

![Orientation 7](../tests/fixtures/images/exif-7.jpg)

![Orientation 8](../tests/fixtures/images/exif-8.jpg)

The test pattern stored on its side in a PNG and in a WebP, upright like
the first one on this page, red at the top left:

![PNG with orientation 6](../tests/fixtures/images/pattern-exif-6.png)

![WebP with orientation 6](../tests/fixtures/images/pattern-exif-6.webp)

## JPEG variants

The lake as a baseline JPEG, a progressive one and a CMYK one, all as wide
as the text and with the same colours, then a file cut off halfway, whose
lower part is grey or smeared:

![Baseline](../tests/fixtures/images/photo-landscape.jpg)

![Progressive](../tests/fixtures/images/photo-progressive.jpg)

![CMYK](../tests/fixtures/images/photo-cmyk.jpg)

![Truncated](../tests/fixtures/images/truncated.jpg)

## PNG variants

Five small PngSuite images of about three cells each: 16-bit grey, an
8-bit palette, 16-bit colour with alpha, interlaced grey, and a palette
with a transparent background:

![16-bit grey](../tests/fixtures/images/pngsuite/basn0g16.png)

![8-bit palette](../tests/fixtures/images/pngsuite/basn3p08.png)

![16-bit RGBA](../tests/fixtures/images/pngsuite/basn6a16.png)

![Interlaced](../tests/fixtures/images/pngsuite/basi0g08.png)

![Palette with transparency](../tests/fixtures/images/pngsuite/tbbn3p08.png)

A lossless WebP of a grey noise pattern:

![Lossless WebP](../tests/fixtures/images/lossless.webp)

## Transparency

A red disc and a half-transparent blue square, with the background showing
around them and through the square:

![Transparent](../tests/fixtures/images/transparent.png)

A compass rose without a background of its own, shrunk to the text width.
Its thin lines and points have clean edges, with no light or dark halo
around them on any background:

![Compass rose](../tests/fixtures/images/compass-rose.png)

## Animations

Animations stand still on their first frame: a red square from a GIF and
from a WebP of red, green and blue frames, a green puzzle piece with eyes
from an animated GIF, and a beach ball from an animated PNG:

![Animated GIF](../tests/fixtures/images/animated.gif)

![Animated WebP](../tests/fixtures/images/animated.webp)

![Jiggy](../tests/fixtures/images/jiggy.gif)

![Beach ball](../tests/fixtures/images/beachball-apng.png)

## SVG

SVG images are drawn at the size they are shown, with sharp edges: the test
pattern without its grid, a sunset in gradients about 36 columns wide, and
a small soccer ball:

![Pattern SVG](../tests/fixtures/images/pattern.svg)

The same pattern again, from a file compressed with gzip:

![Compressed SVG](../tests/fixtures/images/pattern.svgz)

![Sunset icon](../tests/fixtures/images/sunset-icon.svg)

![Soccer ball](../tests/fixtures/images/soccer-ball.svg)

Their text is set in the system's fonts: the forces on an airfoil, labelled
in Greek in a sans-serif font, and a gearset diagram with English labels in
the default serif font. The labels and arrows are black on a transparent
background, as in a browser, so they show only on a light background: a
light terminal, or a theme with a light `background`.

![Forces on an airfoil](../tests/fixtures/images/airfoil-forces.svg)

![Planetary gearset](../tests/fixtures/images/planetary-gearset.svg)

A racetrack in black lines with numbered corners, which also needs a light
background, and a silhouette with softly blurred edges, both as wide as the
text:

![Speedway](../tests/fixtures/images/speedway.svg)

![Silhouette](../tests/fixtures/images/silhouette.svg)

## Lists, quotes and links

Images in a list and a quote start after the bullet and the bar, and the
bar runs beside every row of the image:

- ![Icon in a list](../tests/fixtures/images/icon.png)

- An item with text, then an image lined up with the text:

  ![Pattern in a list](../tests/fixtures/images/pattern.png)

> ![Pattern in a quote](../tests/fixtures/images/pattern.png)

> > ![Panorama in a nested quote](../tests/fixtures/images/panorama.png)

A linked image shows, but clicking it does nothing yet:

[![Linked icon](../tests/fixtures/images/icon.png)](https://example.com)

## Alt text

These show their alt text in brackets instead of an image. In running text,
![an icon](../tests/fixtures/images/icon.png) stays alt text, and so does
[![an icon](../tests/fixtures/images/icon.png) with words](https://example.com)
in a link. So do two images in one paragraph:

![First icon](../tests/fixtures/images/icon.png) ![Second icon](../tests/fixtures/images/icon.png)

### A heading with ![an icon](../tests/fixtures/images/icon.png)

| Image | Note |
|---|---|
| ![Icon in a cell](../tests/fixtures/images/icon.png) | Alt text in a table |

A missing file, a file that is no image, a broken SVG, an image with too
many pixels and a remote image show their alt text, and kiri warns about
each of them on stderr:

![Missing image](../tests/fixtures/images/no-such-file.png)

![Not an image](../tests/fixtures/images/not-an-image.png)

![Broken SVG](../tests/fixtures/images/truncated.svg)

![Huge image](../tests/fixtures/images/huge.png)

![Remote image](https://example.com/image.png)
