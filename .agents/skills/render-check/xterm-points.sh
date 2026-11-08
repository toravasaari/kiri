#!/usr/bin/env bash
# The test points of a terminal that draws sixel, run in xterm: states of the
# pager that must look alike are compared by pixels, so that no picture has to
# be read unless a point fails. A new terminal gets the same points.
#
# Usage: xterm-points.sh [--kiri PATH] [--out DIR] [FILE.md]
#
# FILE.md is a document with images whose first screen holds more than 16
# slices (default: tests/fixtures/images.md). kiri runs with --mode sixel, in
# reading mode, in a window of 1280 x 800 pixels, through render-check.sh.
#
#   scroll   the first screen, and after a page down and a page up
#   panel    the first screen, and after the help opened and closed
#   redraw   the first screen, and after xterm drew its window again from its
#            own memory and the focus came back (CSI I): kiri must have drawn
#            the first screen again. Alone, xterm's own redraw leaves only 16
#            graphics; the point reports how much that costs, which also shows
#            that the screen holds more than 16 slices.
#
# Prints the number of differing pixels for each point and exits with 1 if a
# point that must look alike does not.

set -euo pipefail

here=$(dirname "$(realpath "$0")")
kiri=()
out=
file=tests/fixtures/images.md
while [ $# -gt 0 ]; do
	case $1 in
	--kiri) kiri=(--kiri "$2"); shift 2 ;;
	--out) out=$2; shift 2 ;;
	-*) echo "xterm-points: unknown option $1" >&2; exit 2 ;;
	*) file=$1; shift ;;
	esac
done
[ -n "$out" ] || out=.local/render-check/$(git branch --show-current)/points
mkdir -p "$out"
name=$(date +%F)-$(basename "$file" .md)

# Runs the document in a directory of its own, $1, typing the steps after it
# one by one; the shots are $out/$1/$name-k00.png, -k01.png and so on.
run() {
	local label=$1 args=()
	shift
	rm -rf "${out:?}/$label"
	for step in "$@"; do args+=(--key "$step"); done
	"$here/render-check.sh" --terminal xterm ${kiri[@]+"${kiri[@]}"} \
		--out "$out/$label" "${args[@]}" "$file" -- --mode sixel >/dev/null
}

# The number of pixels that differ between shots $2 and $3 of the run $1.
differ() {
	local result
	result=$(magick compare -metric AE "$out/$1/$name-k$2.png" \
		"$out/$1/$name-k$3.png" null: 2>&1 || true)
	echo "${result%% *}"
}

failed=0
# A point that must look alike.
same() {
	local count
	count=$(differ "$2" "$3" "$4")
	if [ "$count" -eq 0 ]; then
		echo "$1: same"
	else
		echo "$1: DIFFERENT, $count pixels differ"
		failed=1
	fi
}

# A point whose first step must change the screen, or it tests nothing.
changes() {
	local count
	count=$(differ "$2" "$3" "$4")
	if [ "$count" -gt 0 ]; then
		echo "$1: changed, $count pixels differ"
	else
		echo "$1: DID NOT CHANGE, the step did nothing"
		failed=1
	fi
}

run scroll '\x06' '\x02'
changes "scroll: a page down changes the screen" scroll 00 01
same "scroll: a page down and up gives the first screen" scroll 00 02
run panel '?' 'x'
changes "panel: the help opens" panel 00 01
same "panel: the help opened and closed gives the first screen" panel 00 02
run redraw '@redraw' '\x1b[I'
same "redraw: xterm's redraw and a focus-in give the first screen" redraw 00 02
echo "redraw: xterm's redraw alone changes $(differ redraw 00 01) pixels" \
	"(more than 0: the screen holds more than 16 slices)"
exit "$failed"
