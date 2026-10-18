# Fixture images

Images for the image loading tests and for checking image output by eye.
They are committed at full size.

## Made for kiri

[`generate.sh`](generate.sh) makes these with ImageMagick and exiv2. All of
them are dedicated to the public domain under
[CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/), like
`photo-landscape.jpg`, which some are made from.

| File | What it tests |
|---|---|
| `pattern.png` | Test pattern: red, green, blue and yellow quadrants on a grid, 320 × 240 |
| `transparent.png` | Alpha: an opaque disc and a half-transparent square on a transparent background |
| `panorama.png` | A very wide image, 1600 × 200 |
| `portrait.png` | A very tall image, 200 × 800 |
| `icon.png` | A tiny image that must not be enlarged, 16 × 16 |
| `exif-1.jpg` … `exif-8.jpg` | EXIF orientations 1 to 8; every one shows the same upright picture, 600 × 399. From `photo-landscape.jpg` |
| `pattern-exif-6.png`, `pattern-exif-6.webp` | EXIF orientation 6 in a PNG and a WebP: `pattern.png` stored on its side |
| `photo-cmyk.jpg` | A CMYK JPEG, from `photo-landscape.jpg` |
| `photo-progressive.jpg` | A progressive JPEG, from `photo-landscape.jpg` |
| `animated.gif`, `animated.webp` | Three frames, red, green and blue; only the red first frame is shown |
| `pattern.svg` | The test pattern as an SVG of the same size, without the grid; drawn at any size, its quadrants keep sharp edges |
| `pattern.svgz` | `pattern.svg` compressed with gzip |
| `truncated.png`, `truncated.jpg`, `truncated.svg` | The first half of `pattern.png`, `photo-landscape.jpg` and `pattern.svg` |
| `pattern.bmp`, `pattern.tif`, `pattern.qoi`, `pattern.ppm` | `pattern.png` in formats only the full build decodes |
| `transparent.ico` | `transparent.png` as an icon, with its alpha |
| `not-an-image.png` | Text with an image's file name |
| `huge.png` | 20000 × 20000 pixels, more than kiri decodes |

## From elsewhere

From [Wikimedia Commons](https://commons.wikimedia.org/) unless said
otherwise; the links go to each file's description page.

| File | Original | Author | License |
|---|---|---|---|
| `photo-landscape.jpg` | [Cloudscape above Bierstadt Lake, Rocky Mountain National Park, 2009](https://commons.wikimedia.org/wiki/File:Cloudscape_above_Bierstadt_Lake,_Rocky_Mountain_National_Park,_2009.jpg), Commons' 1920 pixels wide rendition | DimiTalen | CC0 1.0 |
| `photo-portrait.jpg` | [Mirror Lake, Yosemite](https://commons.wikimedia.org/wiki/File:MirrorLakeYosemitePortrait.JPG), Commons' 1280 pixels wide rendition | Mutineer | CC0 1.0 |
| `photo-large.jpg` | [Lady fern at Myrstigen trail](https://commons.wikimedia.org/wiki/File:Lady_fern_at_Myrstigen_trail_1.jpg), 4845 × 2980 | W.carter | CC0 1.0 |
| `compass-rose.png` | [Compass rose, transparent](https://commons.wikimedia.org/wiki/File:Compass_rose_transparent.png) | J JMesserly, after Felipe Micaroni Lalli | CC0 1.0 |
| `beachball-apng.png` | [Animated PNG example: bouncing beach ball](https://commons.wikimedia.org/wiki/File:Animated_PNG_example_bouncing_beach_ball.png) | Holger Will | Public domain |
| `jiggy.gif` | [Jiggy](https://commons.wikimedia.org/wiki/File:Jiggy.gif), an animated GIF | Agdm-arthur | CC0 1.0 |
| `lossless.webp` | [Static](https://commons.wikimedia.org/wiki/File:Static.webp), lossless WebP | RytoTechnic | CC0 1.0 |
| `lossy.webp` | [Website accessibility](https://commons.wikimedia.org/wiki/File:Website_accessibility.webp), lossy WebP | Thesweetdonkey | CC0 1.0 |
| `sunset-icon.svg` | [Icon sunset](https://commons.wikimedia.org/wiki/File:Icon_sunset.svg): gradients, sized in millimetres | BirgitLachner | CC0 1.0 |
| `soccer-ball.svg` | [Simple soccer ball (gradient)](https://commons.wikimedia.org/wiki/File:Simple_soccer_ball_(gradient).svg): a small drawing in millimetres | Deltaspace42 | CC0 1.0 |
| `airfoil-forces.svg` | [Aeroforces-el](https://commons.wikimedia.org/wiki/File:Aeroforces-el.svg): Greek text in sans-serif from a style sheet, a gradient and arrow markers | Dipa1965, after [Aeroforces](https://commons.wikimedia.org/wiki/File:Aeroforces.svg) by Amada44 (public domain) | CC0 1.0 |
| `planetary-gearset.svg` | [Simpson Planetary Gearset Schematic Diagram](https://commons.wikimedia.org/wiki/File:Simpson_Planetary_Gearset_Schematic_Diagram.svg): English text in the default font, and a pattern fill | Extemporalist | CC0 1.0 |
| `speedway.svg` | [Sunset Speedway 2025](https://commons.wikimedia.org/wiki/File:Sunset_Speedway_2025.svg): a viewBox without a width or height, and CSS classes | Phillipdh | CC0 1.0 |
| `silhouette.svg` | [Woman sitting sunset silhouette](https://commons.wikimedia.org/wiki/File:Woman-Sitting-Sunset-Silhouette.svg): a blur filter, and only a viewBox | GDJ, from Open Clip Art | CC0 1.0 |
| `pngsuite/*.png` | [PngSuite](http://www.schaik.com/pngsuite/): 16-bit grey, 8-bit palette, 16-bit RGBA, interlaced, and palette with transparency | Willem van Schaik | Free use, see [`pngsuite/LICENSE`](pngsuite/LICENSE) |
