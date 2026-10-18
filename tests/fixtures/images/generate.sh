#!/bin/sh
# Regenerates the self-made fixture images in this directory.
#
# Needs ImageMagick 7 (`magick`) and exiv2. The JPEG variants are derived from
# photo-landscape.jpg, which is committed alongside; see README.md for the
# sources. The output depends a little on the ImageMagick version, so only
# rerun this when a fixture has to change, and commit the results.

set -eu
cd "$(dirname "$0")"

# Test pattern: four coloured quadrants on a black grid, so a wrong
# orientation or channel order shows at a glance and in pixel tests.
magick -size 320x240 xc:black \
    -fill '#ff0000' -draw 'rectangle 0,0 159,119' \
    -fill '#00ff00' -draw 'rectangle 160,0 319,119' \
    -fill '#0000ff' -draw 'rectangle 0,120 159,239' \
    -fill '#ffff00' -draw 'rectangle 160,120 319,239' \
    -fill none -stroke black -strokewidth 1 \
    -draw 'path "M 40,0 V 240 M 80,0 V 240 M 120,0 V 240 M 200,0 V 240 M 240,0 V 240 M 280,0 V 240"' \
    -draw 'path "M 0,40 H 320 M 0,80 H 320 M 0,160 H 320 M 0,200 H 320"' \
    -strip PNG24:pattern.png

# Transparency: an opaque red disc and a half-transparent blue square on a
# fully transparent background.
magick -size 200x200 xc:none \
    -fill '#ff0000' -draw 'circle 100,100 100,30' \
    -fill '#0000ff80' -draw 'rectangle 110,110 190,190' \
    -strip PNG32:transparent.png

# Extreme aspect ratios and a tiny image that must not be enlarged.
magick -size 200x1600 gradient:'#203080-#f0c040' -rotate 90 -strip PNG24:panorama.png
magick -size 200x800 gradient:'#f0c040-#203080' -strip PNG24:portrait.png
magick -size 16x16 pattern:checkerboard -fill '#3070c0' -opaque black \
    -strip PNG24:icon.png

# EXIF orientations 1-8. Each file stores the pixels so that applying its
# orientation tag gives back the upright picture.
magick photo-landscape.jpg -resize 600x -strip upright.ppm
orient() {
    # $2 is deliberately unquoted: it may hold an option and its value.
    # shellcheck disable=SC2086
    magick upright.ppm $2 -quality 92 "exif-$1.jpg"
    exiv2 -M "set Exif.Image.Orientation Short $1" "exif-$1.jpg"
}
orient 1 +repage
orient 2 -flop
orient 3 '-rotate 180'
orient 4 -flip
orient 5 -transpose
orient 6 '-rotate -90'
orient 7 -transverse
orient 8 '-rotate 90'
rm upright.ppm

# EXIF orientation 6 in a PNG and a lossless WebP: the test pattern stored
# on its side.
magick pattern.png -rotate -90 -strip PNG24:pattern-exif-6.png
magick pattern.png -rotate -90 -strip -define webp:lossless=true pattern-exif-6.webp
for file in pattern-exif-6.png pattern-exif-6.webp; do
    exiv2 -M "set Exif.Image.Orientation Short 6" "$file"
done

# JPEG variants of the photo.
magick photo-landscape.jpg -strip -colorspace CMYK -quality 90 photo-cmyk.jpg
magick photo-landscape.jpg -strip -interlace Plane -quality 87 photo-progressive.jpg

# Animation: only the first frame (red) is shown.
magick -delay 50 -size 64x64 xc:'#ff0000' xc:'#00ff00' xc:'#0000ff' \
    -loop 0 animated.gif
magick -delay 50 -size 64x64 xc:'#ff0000' xc:'#00ff00' xc:'#0000ff' \
    -loop 0 -define webp:lossless=true animated.webp

# The test pattern as an SVG of the same size, without the grid: drawn at
# any size, its quadrants keep sharp edges.
cat > pattern.svg <<'EOF'
<svg xmlns="http://www.w3.org/2000/svg" width="320" height="240">
  <rect x="0" y="0" width="160" height="120" fill="#ff0000"/>
  <rect x="160" y="0" width="160" height="120" fill="#00ff00"/>
  <rect x="0" y="120" width="160" height="120" fill="#0000ff"/>
  <rect x="160" y="120" width="160" height="120" fill="#ffff00"/>
</svg>
EOF
gzip -9n < pattern.svg > pattern.svgz

# Broken files: cut short, not an image at all, and too large to decode
# (a 1-bit 20000 x 20000 PNG compresses to a few kilobytes).
half() { head -c "$(($(wc -c < "$1") / 2))" "$1"; }
half pattern.png > truncated.png
half pattern.svg > truncated.svg
half photo-landscape.jpg > truncated.jpg
printf 'This is text, not an image.\n' > not-an-image.png
magick -size 20000x20000 xc:white -depth 1 -strip PNG:huge.png

# The formats only the full build decodes, each holding the test pattern
# or the transparent image without loss.
magick pattern.png -strip BMP3:pattern.bmp
magick pattern.png -strip -compress zip pattern.tif
magick pattern.png -strip pattern.qoi
magick pattern.png -strip PPM:pattern.ppm
magick pattern.png -strip -compress RLE pattern.tga
magick transparent.png -strip transparent.ico
