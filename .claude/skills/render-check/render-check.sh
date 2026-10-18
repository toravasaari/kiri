#!/usr/bin/env bash
# Render Markdown documents with kiri in kitty on a virtual X display and save
# a screenshot of each, so that output can be reviewed without a real window.
#
# Usage: render-check.sh [options] FILE.md... [-- KIRI_ARGS...]
#
#   --out DIR            where screenshots go (default:
#                        .local/render-check/BRANCH, or the commit when detached)
#   --size WxH           window and screen size in pixels (default: 1280x16000,
#                        and llvmpipe draws at most 16384 pixels high)
#   --page-height PX     also slice screenshots into pages this tall
#                        (default: 1600; 0 turns slicing off)
#   --kitty-config FILE  kitty config to use (default: none, kitty defaults)
#   --kiri PATH          kiri binary (default: build target/debug/kiri)
#   --timeout SECONDS    how long kiri may take for one document (default: 120)
#
# Each FILE.md gives OUT/DATE-NAME.png, cropped below the last drawn row, and,
# when it is taller than a page, OUT/DATE-NAME-p01.png, OUT/DATE-NAME-p02.png
# and so on. DATE is today's, such as 2026-10-18.
# If kiri fails or crashes on a document, its screenshot is still saved and
# the script exits with kiri's status after the other documents.
# Needs Xvfb, kitty, ImageMagick (import, magick) and Mesa's llvmpipe.

set -euo pipefail

out=
size=1280x16000
page_height=1600
kitty_config=NONE
kiri=
timeout=120
files=()
kiri_args=()

die() {
	echo "render-check: $*" >&2
	exit 1
}

while [ $# -gt 0 ]; do
	case $1 in
	--out) out=$2; shift 2 ;;
	--size) size=$2; shift 2 ;;
	--page-height) page_height=$2; shift 2 ;;
	--kitty-config) kitty_config=$2; shift 2 ;;
	--kiri) kiri=$2; shift 2 ;;
	--timeout) timeout=$2; shift 2 ;;
	--) shift; kiri_args=("$@"); break ;;
	-h | --help) sed -n '2,/^$/s/^# \{0,1\}//p' "$0"; exit 0 ;;
	-*) die "unknown option $1" ;;
	*) files+=("$1"); shift ;;
	esac
done

[ ${#files[@]} -gt 0 ] || die "no documents given; see --help"
for file in "${files[@]}"; do
	[ -f "$file" ] || die "$file: no such file"
done
[[ $size =~ ^([0-9]+)x([0-9]+)$ ]] || die "--size must look like 1280x16000"
width=${BASH_REMATCH[1]}
height=${BASH_REMATCH[2]}
[[ $timeout =~ ^[1-9][0-9]*$ ]] || die "--timeout must be whole seconds"

missing=()
for tool in Xvfb kitty import magick; do
	command -v "$tool" >/dev/null || missing+=("$tool")
done
[ ${#missing[@]} -eq 0 ] || die "missing ${missing[*]}" \
	"(Fedora: xorg-x11-server-Xvfb, kitty, ImageMagick)"

if [ -z "$kiri" ]; then
	cargo build --quiet
	kiri=$PWD/target/debug/kiri
fi
kiri=$(realpath "$kiri")
# .local is shared with jailbee containers, so the host sees their shots.
if [ -z "$out" ]; then
	branch=$(git branch --show-current 2>/dev/null || true)
	[ -n "$branch" ] || branch=$(git rev-parse --short HEAD)
	out=.local/render-check/$branch
fi
mkdir -p "$out"
day=$(date +%F)

work=$(mktemp -d)
xvfb_pid=
kitty_pid=
cleanup() {
	[ -n "$kitty_pid" ] && kill "$kitty_pid" 2>/dev/null
	[ -n "$xvfb_pid" ] && kill "$xvfb_pid" 2>/dev/null
	wait 2>/dev/null
	rm -rf "$work"
}
trap cleanup EXIT

# A display of its own: -displayfd picks a free number and reports it once
# the server accepts connections. A fixed DPI keeps the cell size the same
# from run to run.
exec 3>"$work/display"
Xvfb -displayfd 3 -screen 0 "${width}x${height}x24" -dpi 96 -nolisten tcp \
	>"$work/xvfb.log" 2>&1 &
xvfb_pid=$!
exec 3>&-
for _ in $(seq 100); do
	[ -s "$work/display" ] && break
	kill -0 "$xvfb_pid" 2>/dev/null || break
	sleep 0.1
done
[ -s "$work/display" ] || { cat "$work/xvfb.log" >&2; die "Xvfb did not start"; }
display=:$(cat "$work/display")

# Runs inside kitty: "$0" is the file to report to, the rest is kiri's
# command line. kitty is slow to answer while it starts, so kiri's probe
# could time out and fall back to text; an answer to a query first means
# kitty is ready, and without one kiri is not started. kitty answers in
# order, so when it reports the cursor position after kiri, it has read all
# of kiri's output. The report is kiri's exit status, the cursor's row and
# column and the cell size in pixels.
inner='ask() { printf "%b" "$1"; IFS= read -r -s -d "$2" -t 60 reply; }
ask "\033[c" c || { echo "no answer" >"$0"; exit 1; }
"$@"
status=$?
ask "\033[6n" R; position=${reply##*\[}
ask "\033[16t" t; cell=${reply##*\[6;}
echo "$status ${position/;/ } ${cell/;/ }" >"$0"
exec sleep 600'

# Whether kitty draws the cursor at the cell where kiri's output ended: the
# cell differs from its neighbour on the same, otherwise empty, line. kitty
# moves the cursor there only after all of kiri's output, so a screenshot
# that shows it has the whole document, unless the document scrolled.
# magick compare exits with 0 when the cells are equal, 1 when they differ
# and 2 when it cannot compare them, which stops the script.
cursor_drawn() {
	local shot=$1 x=$(((col - 1) * cell_w)) y=$(((row - 1) * cell_h)) other
	local status=0
	if [ $((x + 2 * cell_w)) -le "$width" ]; then
		other=$((x + cell_w))
	else
		other=$((x - cell_w))
	fi
	magick compare -metric AE "$shot[${cell_w}x${cell_h}+$x+$y]" \
		"$shot[${cell_w}x${cell_h}+$other+$y]" null: \
		2>"$work/compare.log" || status=$?
	case $status in
	0) return 1 ;;
	1) return 0 ;;
	*)
		cat "$work/compare.log" >&2
		die "cannot compare the cursor's cell (magick compare status $status)"
		;;
	esac
}

# Take screenshots until the cursor shows that kitty has drawn all of kiri's
# output, and two of those in a row are equal. kitty can take seconds to
# draw large images with the software renderer, more on a busy machine,
# and meanwhile shows the last frame it drew.
settle() {
	local shot=$1 prev=$work/prev.ppm deadline=$((SECONDS + 60))
	rm -f "$prev"
	while [ "$SECONDS" -lt "$deadline" ]; do
		sleep 0.25
		DISPLAY=$display import -window root "$shot"
		if ! cursor_drawn "$shot"; then
			rm -f "$prev"
			continue
		fi
		if [ -e "$prev" ] && cmp -s "$shot" "$prev"; then
			return 0
		fi
		cp "$shot" "$prev"
	done
	echo "render-check: screen did not settle; saved the last screenshot" >&2
}

failed=0
for file in "${files[@]}"; do
	name=$day-$(basename "$file" .md)
	shot=$out/$name.png
	rm -f "$work/done" "$out/$name"-p[0-9][0-9].png

	# The child must not talk to the kitty this script may run in, and kitty
	# must use X11 even when a Wayland session is around.
	env -u WAYLAND_DISPLAY -u KITTY_WINDOW_ID -u KITTY_PID \
		-u KITTY_LISTEN_ON -u KITTY_PUBLIC_KEY -u KITTY_INSTALLATION_DIR \
		DISPLAY="$display" \
		kitty --config "$kitty_config" -o linux_display_server=x11 \
		-o remember_window_size=no -o cursor_blink_interval=0 \
		-o initial_window_width="$width" -o initial_window_height="$height" \
		bash -c "$inner" "$work/done" "$kiri" "${kiri_args[@]}" "$file" \
		>"$work/kitty.log" 2>&1 &
	kitty_pid=$!

	deadline=$((SECONDS + timeout))
	while [ ! -s "$work/done" ] && [ "$SECONDS" -lt "$deadline" ]; do
		kill -0 "$kitty_pid" 2>/dev/null || break
		sleep 0.1
	done
	if [ ! -s "$work/done" ]; then
		cat "$work/kitty.log" >&2
		die "$file: kiri did not finish in kitty within $timeout s" \
			"(--timeout)"
	fi
	[ "$(cat "$work/done")" != "no answer" ] ||
		die "$file: kitty did not answer the device attributes query" \
			"within 60 s, so kiri was not started"
	read -r status row col cell_h cell_w <"$work/done"
	[[ "$status $row $col $cell_h $cell_w" =~ ^[0-9]+( [1-9][0-9]*){4}$ ]] ||
		die "$file: kitty did not report the cursor and cell size" \
			"($(cat "$work/done"))"
	if [ "$status" -gt 128 ]; then
		echo "render-check: $file: kiri was killed by signal" \
			"$((status - 128)) (status $status)" >&2
	elif [ "$status" -ne 0 ]; then
		echo "render-check: $file: kiri exited with status $status" >&2
	fi
	if [ "$status" -ne 0 ] && [ "$failed" -eq 0 ]; then
		failed=$status
	fi
	settle "$work/full.ppm"
	kill "$kitty_pid" 2>/dev/null
	wait "$kitty_pid" 2>/dev/null || true
	kitty_pid=

	# Crop below the last row that differs from the background.
	box=$(magick "$work/full.ppm" -fuzz 2% -format '%@' info:)
	[[ $box =~ ^[0-9]+x([0-9]+)\+[0-9]+\+([0-9]+)$ ]] ||
		die "$file: cannot measure the screenshot ($box)"
	bottom=$((BASH_REMATCH[1] + BASH_REMATCH[2]))
	[ "$bottom" -ge "$height" ] &&
		echo "render-check: $file fills the screen;" \
			"raise the height of --size to see all of it" >&2
	magick "$work/full.ppm" -crop "${width}x${bottom}+0+0" +repage "$shot"
	echo "$shot"

	if [ "$page_height" -gt 0 ] && [ "$bottom" -gt "$page_height" ]; then
		magick "$shot" -crop "${width}x${page_height}" +repage \
			-scene 1 "$out/$name-p%02d.png"
		ls "$out/$name"-p[0-9][0-9].png
	fi
done
exit "$failed"
