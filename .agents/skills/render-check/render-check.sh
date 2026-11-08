#!/usr/bin/env bash
# Render Markdown documents with kiri in kitty, Ghostty or xterm on a virtual X
# display and save a screenshot of each, so that output can be reviewed
# without a real window.
#
# Usage: render-check.sh [options] FILE.md... [-- KIRI_ARGS...]
#
#   --out DIR            where screenshots go (default:
#                        .local/render-check/BRANCH, or the commit when detached)
#   --terminal NAME      kitty (default), ghostty or xterm (with -ti vt340,
#                        so that it has sixel)
#   --size WxH           window and screen size in pixels (default: 1280x16000,
#                        and llvmpipe draws at most 16384 pixels high; for
#                        Ghostty 1280x5900, as it fails above about 6000;
#                        xterm sizes its window in whole cells that fit it)
#   --page-height PX     also slice screenshots into pages this tall
#                        (default: 1600; 0 turns slicing off)
#   --kitty-config FILE  kitty config to use (default: none, kitty defaults);
#                        Ghostty gets a config of the script's own
#   --kiri PATH          kiri binary (default: build target/debug/kiri)
#   --timeout SECONDS    how long kiri may take for one document (default: 120)
#   --key TEXT           open the pager and type TEXT, after a screenshot of
#                        its first frame; each --key is one step, with a
#                        screenshot after it, and q ends the pager. TEXT takes
#                        Python's escapes, such as \x1b[C for the right arrow.
#                        kitty and xterm; the window is 1280x800 unless --size
#                        says otherwise.
#                        The step @redraw (xterm only) covers and uncovers the
#                        window as --redraw does, once, instead of typing.
#   --redraw             xterm only: before each screenshot, cover the window
#                        and uncover it, so that xterm draws it again from its
#                        own memory, as after a restore from minimised. A
#                        screenshot then shows what xterm kept, not only what
#                        was drawn.
#
# Each FILE.md gives OUT/DATE-NAME.png, cropped below the last drawn row, and,
# when it is taller than a page, OUT/DATE-NAME-p01.png, OUT/DATE-NAME-p02.png
# and so on. DATE is today's, such as 2026-10-18. With --key, it gives the
# pager's frames instead, whole: OUT/DATE-NAME-k00.png before the first key,
# OUT/DATE-NAME-k01.png after it, and so on.
# If kiri fails or crashes on a document, its screenshot is still saved and
# the script exits with kiri's status after the other documents.
# Needs Xvfb, kitty, Ghostty or xterm, ImageMagick (import, magick) and Mesa's
# llvmpipe; Ghostty also dbus-run-session, xterm also xwininfo and python3.

set -euo pipefail

out=
terminal=kitty
size=
page_height=1600
kitty_config=
kiri=
timeout=120
files=()
kiri_args=()
keys=()
redraw=0

die() {
	echo "render-check: $*" >&2
	exit 1
}

while [ $# -gt 0 ]; do
	case $1 in
	--out) out=$2; shift 2 ;;
	--terminal) terminal=$2; shift 2 ;;
	--size) size=$2; shift 2 ;;
	--page-height) page_height=$2; shift 2 ;;
	--kitty-config) kitty_config=$2; shift 2 ;;
	--kiri) kiri=$2; shift 2 ;;
	--timeout) timeout=$2; shift 2 ;;
	--key) keys+=("$2"); shift 2 ;;
	--redraw) redraw=1; shift ;;
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
case $terminal in
kitty)
	if [ ${#keys[@]} -gt 0 ]; then
		[ -n "$size" ] || size=1280x800
	fi
	[ -n "$size" ] || size=1280x16000
	[ -n "$kitty_config" ] || kitty_config=NONE
	;;
xterm)
	if [ ${#keys[@]} -gt 0 ]; then
		[ -n "$size" ] || size=1280x800
	fi
	[ -n "$size" ] || size=1280x16000
	[ -z "$kitty_config" ] || die "--kitty-config is for kitty only"
	;;
ghostty)
	[ -n "$size" ] || size=1280x5900
	[ -z "$kitty_config" ] || die "--kitty-config is for kitty only"
	[ ${#keys[@]} -eq 0 ] || die "--key is for kitty and xterm only"
	;;
*) die "--terminal must be kitty, ghostty or xterm" ;;
esac
[ "$redraw" -eq 0 ] || [ "$terminal" = xterm ] || die "--redraw is for xterm only"
for key in "${keys[@]}"; do
	[ "$key" != @redraw ] || [ "$terminal" = xterm ] ||
		die "the step @redraw is for xterm only"
done
[[ $size =~ ^([0-9]+)x([0-9]+)$ ]] || die "--size must look like 1280x16000"
width=${BASH_REMATCH[1]}
height=${BASH_REMATCH[2]}
[[ $timeout =~ ^[1-9][0-9]*$ ]] || die "--timeout must be whole seconds"

missing=()
tools=(Xvfb "$terminal" import magick)
case $terminal in
ghostty) tools+=(dbus-run-session) ;;
xterm) tools+=(xwininfo python3) ;;
esac
for tool in "${tools[@]}"; do
	command -v "$tool" >/dev/null || missing+=("$tool")
done
[ ${#missing[@]} -eq 0 ] || die "missing ${missing[*]}" \
	"(Fedora: xorg-x11-server-Xvfb, kitty, ghostty or xterm, ImageMagick," \
	"dbus, xwininfo, python3)"

if [ -z "$kiri" ]; then
	cargo build --quiet
	kiri=$PWD/target/debug/kiri
fi
kiri=$(realpath "$kiri")
# Runs kiri in xterm on a pseudo-terminal of its own, beside this script.
relay=$(dirname "$(realpath "$0")")/xterm-relay.py
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
term_pid=
# Stops the terminal: Ghostty runs under dbus-run-session, which does not
# stop it when killed itself.
# Either may have exited already.
stop_terminal() {
	pkill -P "$term_pid" 2>/dev/null || true
	kill "$term_pid" 2>/dev/null || true
	wait "$term_pid" 2>/dev/null || true
	term_pid=
}
cleanup() {
	[ -z "$term_pid" ] || stop_terminal
	[ -z "$xvfb_pid" ] || kill "$xvfb_pid" 2>/dev/null || true
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

# xterm: its options, and a window of its size in whole cells that fits the
# screen. The internal border is set, not left to the default, so that the
# cell is the window's size less the border on each side.
xterm_opts=(-ti vt340 -fa Monospace -fs 11 +sb -bg '#000000' -fg '#dddddd'
	-xrm 'XTerm*internalBorder: 2')
xterm_border=2
# Where the first cell starts, from the top left of the window.
inset=0
# Prints the width and height of the window named $1 once it is viewable.
xterm_window_size() {
	local i out
	for i in $(seq 100); do
		out=$(DISPLAY=$display xwininfo -name "$1" 2>/dev/null) || true
		if [[ $out == *IsViewable* ]]; then
			awk '/Width:/ { w = $2 } /Height:/ { h = $2 } END { print w, h }' \
				<<<"$out"
			return 0
		fi
		sleep 0.1
	done
	return 1
}
xterm_cols=
xterm_rows=
if [ "$terminal" = xterm ]; then
	# A window of one cell, which cat keeps open until it is killed.
	DISPLAY=$display xterm "${xterm_opts[@]}" -geometry 1x1+0+0 \
		-title render-check-probe -e cat &
	probe_pid=$!
	read -r probe_w probe_h < <(xterm_window_size render-check-probe) ||
		die "xterm did not show its probe window"
	pkill -P "$probe_pid" 2>/dev/null || true
	kill "$probe_pid" 2>/dev/null || true
	wait "$probe_pid" 2>/dev/null || true
	xterm_cols=$(((width - 2 * xterm_border) / (probe_w - 2 * xterm_border)))
	xterm_rows=$(((height - 2 * xterm_border) / (probe_h - 2 * xterm_border)))
	[ "$xterm_cols" -ge 10 ] && [ "$xterm_rows" -ge 5 ] ||
		die "--size $size is too small for xterm"
	inset=$xterm_border
fi

# Runs inside the terminal: "$0" is the file to report to, the rest is kiri's
# command line. kitty is slow to answer while it starts, so kiri's probe
# could time out and fall back to text; an answer to a query first means
# kitty is ready, and without one kiri is not started. Ghostty answers at a
# smaller window size for a second or two before it takes the size asked
# for, so with WAIT_FOR_SIZE set kiri starts once the size stays the same.
# The terminal answers in order, so when it reports the cursor position
# after kiri, it has read all of kiri's output. The report is kiri's exit
# status, the cursor's row and column and the cell size in pixels. Answers
# kiri left unread mean its probe timed out, which a busy machine can cause;
# they would be taken for the answers asked for here.
inner='ask() { printf "%b" "$1"; IFS= read -r -s -d "$2" -t 60 reply; }
ask "\033[c" c || { echo "no answer" >"$0"; exit 1; }
if [ -n "${WAIT_FOR_SIZE:-}" ]; then
	sleep 2
	last=
	for _ in $(seq 20); do
		ask "\033[18t" t
		[ "$reply" = "$last" ] && break
		last=$reply
		sleep 0.5
	done
fi
"$@"
status=$?
IFS= read -r -s -d "" -t 0.5 unread
[ -z "$unread" ] || { echo "unread" >"$0"; exit 1; }
ask "\033[6n" R; position=${reply##*\[}
if [ -n "${RENDER_CHECK_RELAY:-}" ]; then
	# xterm answers no window operation: the cell is what kiri works out from
	# the window size, pixels included. (The name must not start with XTERM_,
	# which xterm sets and clears for its shell.)
	cell=$(python3 "$RENDER_CHECK_RELAY" --cell)
else
	ask "\033[16t" t; cell=${reply##*\[6;}
fi
echo "$status ${position/;/ } ${cell/;/ }" >"$0"
exec sleep 600'

# Whether kitty draws the cursor at the cell where kiri's output ended: the
# cell differs from its neighbour on the same, otherwise empty, line. kitty
# moves the cursor there only after all of kiri's output, so a screenshot
# that shows it has the whole document, unless the document scrolled.
# magick compare exits with 0 when the cells are equal, 1 when they differ
# and 2 when it cannot compare them, which stops the script.
cursor_drawn() {
	local shot=$1 other
	local x=$(((col - 1) * cell_w + inset)) y=$(((row - 1) * cell_h + inset))
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

# Makes xterm draw the window again from its own memory: a window of the same
# size covers it and goes again, so that xterm gets exposure events, as it
# does after a restore from minimised. A screenshot then shows what xterm kept
# and not only what was drawn.
force_redraw() {
	DISPLAY=$display xterm "${xterm_opts[@]}" \
		-geometry "$((xterm_cols + 2))x$((xterm_rows + 2))+0+0" \
		-title render-check-cover -e cat &
	local cover_pid=$!
	xterm_window_size render-check-cover >/dev/null || true
	sleep 0.3
	pkill -P "$cover_pid" 2>/dev/null || true
	kill "$cover_pid" 2>/dev/null || true
	wait "$cover_pid" 2>/dev/null || true
	sleep 0.8
}

# Takes a screenshot of the whole display into $1, after a forced redraw of
# xterm with --redraw.
grab() {
	[ "$redraw" -eq 0 ] || force_redraw
	DISPLAY=$display import -window root "$1"
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
		grab "$shot"
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

# With --key, the pager's frame after waiting $2 seconds: three screenshots
# in a row, half a second apart, that are equal, as the pager hides the
# cursor that settle waits for, and kitty's software renderer can take a
# while to draw the images of a frame.
settle_frame() {
	local shot=$1 prev=$work/prev.ppm deadline=$((SECONDS + 60)) same=0
	rm -f "$prev"
	sleep "$2"
	while [ "$SECONDS" -lt "$deadline" ]; do
		grab "$shot"
		if [ -e "$prev" ] && cmp -s "$shot" "$prev"; then
			same=$((same + 1))
			[ "$same" -lt 2 ] || return 0
		else
			same=0
		fi
		cp "$shot" "$prev"
		sleep 0.5
	done
	echo "render-check: the pager did not settle; saved the last screenshot" >&2
}

# Waits until the pager's status line shows on the screen, which a slow first
# layout can delay by many seconds, up to $timeout. kitty is asked for the
# screen's text; for xterm the relay watches the output for the status line.
wait_for_pager() {
	local deadline=$((SECONDS + timeout))
	while [ "$SECONDS" -lt "$deadline" ]; do
		if [ "$terminal" = xterm ]; then
			[ -e "$work/pager" ] && return 0
		elif kitty @ --to "unix:$work/kitty.sock" get-text 2>/dev/null |
			grep -q 'q quit'; then
			return 0
		fi
		sleep 0.5
	done
	die "the pager did not show its status line within $timeout s (--timeout)"
}

# Types $1 into the terminal, as the reader would: through kitty's remote
# control, or into the FIFO that the xterm relay reads from. The FIFO is opened
# for reading and writing, so that the write does not wait for a relay that
# has exited.
type_keys() {
	if [ "$terminal" = xterm ]; then
		printf '%b' "$1" 1<>"$work/keys" || die "xterm did not take the keys $1"
		return
	fi
	kitty @ --to "unix:$work/kitty.sock" send-text -- "$1" \
		>>"$work/terminal.log" 2>&1 ||
		die "kitty did not take the keys $1"
}

# A Ghostty configuration of the script's own, so that the maintainer's
# does not apply: a window of the size asked for, in cells of about 9 x 20
# pixels at 11 pt and 96 dpi, without decorations or a blinking cursor.
ghostty_config() {
	cat <<-EOF
		gtk-single-instance = false
		linux-cgroup = never
		shell-integration = none
		confirm-close-surface = false
		window-decoration = false
		gtk-adwaita = false
		font-size = 11
		cursor-style-blink = false
		window-width = $(((width - 20) / 9))
		window-height = $(((height - 40) / 20))
	EOF
}

# Starts the terminal on the virtual display with kiri rendering $1 in it.
# The child must not talk to the terminal this script may run in, and the
# terminal must use X11 even when a Wayland session is around. kiri prints
# rather than pages, unless --key is given and the script types into the
# pager, through kitty's remote control or the xterm relay's FIFO; -p after --
# still opens it. COLORTERM is unset, as xterm and a plain desktop session
# set none, so that kiri must find 24-bit colour by asking.
start_terminal() {
	local paging=never remote=()
	if [ ${#keys[@]} -gt 0 ]; then
		paging=always
		remote=(-o allow_remote_control=yes --listen-on "unix:$work/kitty.sock")
	fi
	local env=(env -u WAYLAND_DISPLAY -u KITTY_WINDOW_ID -u KITTY_PID
		-u KITTY_LISTEN_ON -u KITTY_PUBLIC_KEY -u KITTY_INSTALLATION_DIR
		-u TMUX -u TMUX_PANE -u TERM_PROGRAM -u TERM_PROGRAM_VERSION
		-u GHOSTTY_RESOURCES_DIR -u GHOSTTY_BIN_DIR -u GHOSTTY_SHELL_FEATURES
		-u COLORTERM DISPLAY="$display" KIRI_PAGING=$paging)
	case $terminal in
	kitty)
		"${env[@]}" kitty --config "$kitty_config" "${remote[@]}" \
			-o linux_display_server=x11 \
			-o remember_window_size=no -o cursor_blink_interval=0 \
			-o initial_window_width="$width" \
			-o initial_window_height="$height" \
			bash -c "$inner" "$work/done" "$kiri" "${kiri_args[@]}" "$1" \
			>"$work/terminal.log" 2>&1 &
		;;
	xterm)
		# The relay runs kiri on a pseudo-terminal of its own, so that the
		# keys and the size reach kiri as they would from a terminal.
		"${env[@]}" RENDER_CHECK_RELAY="$relay" xterm "${xterm_opts[@]}" \
			-geometry "${xterm_cols}x${xterm_rows}+0+0" -title render-check \
			-e bash -c "$inner" "$work/done" python3 "$relay" \
			--keys "$work/keys" --ready "$work/pager" -- \
			"$kiri" "${kiri_args[@]}" "$1" >"$work/terminal.log" 2>&1 &
		;;
	ghostty)
		# Ghostty's -e runs its arguments through the shell joined into
		# one line, so the command goes in a script of its own. Its own
		# D-Bus session keeps it away from a Ghostty already running.
		ghostty_config >"$work/ghostty.conf"
		printf '%s\n' "$inner" >"$work/inner.sh"
		{
			printf 'exec bash -c "$(cat %q)"' "$work/inner.sh"
			printf ' %q' "$work/done" "$kiri" "${kiri_args[@]}" "$1"
			echo
		} >"$work/launch.sh"
		"${env[@]}" GDK_BACKEND=x11 LIBGL_ALWAYS_SOFTWARE=1 GTK_A11Y=none \
			WAIT_FOR_SIZE=1 dbus-run-session -- \
			ghostty --config-default-files=false \
			--config-file="$work/ghostty.conf" -e bash "$work/launch.sh" \
			>"$work/terminal.log" 2>&1 &
		;;
	esac
	term_pid=$!
}

failed=0
for file in "${files[@]}"; do
	name=$day-$(basename "$file" .md)
	shot=$out/$name.png
	rm -f "$work/done" "$work/pager" "$out/$name"-p[0-9][0-9].png
	# A new FIFO for each document, so that keys left over from one cannot
	# reach the next.
	rm -f "$work/keys"
	mkfifo "$work/keys"

	start_terminal "$file"

	if [ ${#keys[@]} -gt 0 ]; then
		rm -f "$out/$name"-k[0-9][0-9].png
		if [ "$terminal" = kitty ]; then
			# kitty listens once it has started.
			deadline=$((SECONDS + timeout))
			while [ ! -S "$work/kitty.sock" ] && [ "$SECONDS" -lt "$deadline" ]; do
				sleep 0.1
			done
			[ -S "$work/kitty.sock" ] || die "$file: kitty did not listen for keys"
		fi
		# The pager's first frame, once its status line shows and the
		# screen stays the same.
		wait_for_pager
		settle_frame "$work/full.ppm" 1
		magick "$work/full.ppm" "$out/$name-k00.png"
		echo "$out/$name-k00.png"
		step=0
		for key in "${keys[@]}"; do
			step=$((step + 1))
			if [ "$key" = @redraw ]; then
				# xterm draws the window again, and the screenshot shows
				# what it kept.
				force_redraw
				sleep 0.5
				DISPLAY=$display import -window root "$work/full.ppm"
			else
				type_keys "$key"
				settle_frame "$work/full.ppm" 0.5
			fi
			printf -v shot '%s/%s-k%02d.png' "$out" "$name" "$step"
			magick "$work/full.ppm" "$shot"
			echo "$shot"
		done
		type_keys q
	fi

	deadline=$((SECONDS + timeout))
	while [ ! -s "$work/done" ] && [ "$SECONDS" -lt "$deadline" ]; do
		kill -0 "$term_pid" 2>/dev/null || break
		sleep 0.1
	done
	if [ ! -s "$work/done" ]; then
		cat "$work/terminal.log" >&2
		die "$file: kiri did not finish in $terminal within $timeout s" \
			"(--timeout)"
	fi
	[ "$(cat "$work/done")" != "no answer" ] ||
		die "$file: $terminal did not answer the device attributes query" \
			"within 60 s, so kiri was not started"
	[ "$(cat "$work/done")" != "unread" ] ||
		die "$file: kiri left $terminal's answers unread, so its probe timed" \
			"out, probably because the machine is busy; run again when idle"
	read -r status row col cell_h cell_w <"$work/done"
	[[ "$status $row $col $cell_h $cell_w" =~ ^[0-9]+( [1-9][0-9]*){4}$ ]] ||
		die "$file: $terminal did not report the cursor and cell size" \
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
	if [ ${#keys[@]} -gt 0 ]; then
		stop_terminal
		continue
	fi
	settle "$work/full.ppm"
	stop_terminal

	# Crop below the last row that differs from the background.
	box=$(magick "$work/full.ppm" -fuzz 2% -format '%@' info:)
	[[ $box =~ ^[0-9]+x([0-9]+)\+[0-9]+\+([0-9]+)$ ]] ||
		die "$file: cannot measure the screenshot ($box)"
	bottom=$((BASH_REMATCH[1] + BASH_REMATCH[2]))
	if [ "$bottom" -ge "$height" ]; then
		if [ "$terminal" = ghostty ]; then
			echo "render-check: $file fills the screen, and Ghostty" \
				"fails above about 6000 pixels: check a shorter part" >&2
		else
			echo "render-check: $file fills the screen;" \
				"raise the height of --size to see all of it" >&2
		fi
	fi
	magick "$work/full.ppm" -crop "${width}x${bottom}+0+0" +repage "$shot"
	echo "$shot"

	if [ "$page_height" -gt 0 ] && [ "$bottom" -gt "$page_height" ]; then
		magick "$shot" -crop "${width}x${page_height}" +repage \
			-scene 1 "$out/$name-p%02d.png"
		ls "$out/$name"-p[0-9][0-9].png
	fi
done
exit "$failed"
