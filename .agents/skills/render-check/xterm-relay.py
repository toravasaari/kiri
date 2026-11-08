#!/usr/bin/env python3
# Runs a command on a pseudo-terminal of its own, for render-check's xterm
# runs. The command's terminal is the one this runs in, so the command sees
# the window's size, pixels included, and the answers xterm sends. Output and
# answers pass through unchanged. Bytes read from --keys go to the command as
# typed keys, and --ready FILE is created once the output shows the pager's
# status line.
#
# Usage: xterm-relay.py [--keys FIFO] [--ready FILE] -- COMMAND [ARG...]
#        xterm-relay.py --cell
#
# Exits with the command's status, or 128 plus the number of the signal that
# killed it. --cell prints the height and width of a cell in pixels, as kiri
# works them out from the size of the terminal on standard input.

import fcntl
import os
import pty
import re
import select
import signal
import struct
import sys
import termios
import tty

# Escape sequences that the status line check leaves out: the pager writes
# its keys in colour, so `q quit` may have escapes between its parts.
ESCAPE = re.compile(
    rb"\x1b\[[0-?]*[ -/]*[@-~]"
    rb"|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)"
    rb"|\x1b[()][0-9A-Za-z]"
    rb"|\x1b[@-Z\\-_]"
)


def usage(message):
    sys.exit(f"xterm-relay: {message}\n"
             "usage: xterm-relay.py [--keys FIFO] [--ready FILE] -- COMMAND [ARG...]\n"
             "       xterm-relay.py --cell")


def copy_size(master):
    """Gives the command's terminal the size of ours, pixels included."""
    size = fcntl.ioctl(0, termios.TIOCGWINSZ, bytes(8))
    fcntl.ioctl(master, termios.TIOCSWINSZ, size)


def cell_size():
    """The cell's height and width in pixels, from the terminal's size."""
    rows, cols, xpixels, ypixels = struct.unpack(
        "HHHH", fcntl.ioctl(0, termios.TIOCGWINSZ, bytes(8)))
    return ypixels // rows, xpixels // cols


def write_all(fd, data):
    while data:
        data = data[os.write(fd, data):]


def relay(master, fifo, ready):
    """Copies bytes between the terminal and the command until it exits."""
    watch = [master, 0] + ([fifo] if fifo is not None else [])
    tail = b""
    while True:
        readable, _, _ = select.select(watch, [], [])
        if master in readable:
            try:
                data = os.read(master, 65536)
            except OSError:  # EIO: the command has closed its terminal
                break
            if not data:
                break
            write_all(1, data)
            if ready and not os.path.exists(ready):
                tail = (tail + ESCAPE.sub(b"", data))[-256:]
                if b"q quit" in tail:
                    open(ready, "w").close()
        if 0 in readable:
            try:
                data = os.read(0, 65536)
            except OSError:
                data = b""
            if data:
                send(master, data)
            else:
                watch.remove(0)
        if fifo is not None and fifo in readable:
            send(master, os.read(fifo, 65536))


def send(master, data):
    """Types data into the command; once it has exited there is nobody to
    take it, and the output still has to be read to its end."""
    try:
        write_all(master, data)
    except OSError:
        pass


def main(argv):
    if argv[1:] == ["--cell"]:
        print(*cell_size())
        return
    keys = ready = None
    args = argv[1:]
    while args and args[0] != "--":
        if len(args) < 2 or args[0] not in ("--keys", "--ready"):
            usage(f"unknown option {args[0]}")
        if args[0] == "--keys":
            keys = args[1]
        else:
            ready = args[1]
        args = args[2:]
    command = args[1:]
    if not command:
        usage("no command given")

    master, slave = pty.openpty()
    if os.isatty(0):
        copy_size(master)
    pid = os.fork()
    if pid == 0:
        os.close(master)
        os.setsid()
        fcntl.ioctl(slave, termios.TIOCSCTTY, 0)
        for fd in (0, 1, 2):
            os.dup2(slave, fd)
        if slave > 2:
            os.close(slave)
        # Python ignores SIGPIPE, and the command would inherit that.
        signal.signal(signal.SIGPIPE, signal.SIG_DFL)
        try:
            os.execvp(command[0], command)
        except OSError as error:
            os.write(2, f"xterm-relay: {command[0]}: {error}\n".encode())
            os._exit(127)
    os.close(slave)

    saved = None
    if os.isatty(0):
        # Raw, so that the command's bytes reach the terminal unchanged.
        saved = termios.tcgetattr(0)
        tty.setraw(0)
        signal.signal(signal.SIGWINCH, lambda signum, frame: copy_size(master))
    fifo = os.open(keys, os.O_RDWR) if keys else None
    try:
        relay(master, fifo, ready)
    finally:
        if saved is not None:
            termios.tcsetattr(0, termios.TCSADRAIN, saved)
    _, status = os.waitpid(pid, 0)
    if os.WIFSIGNALED(status):
        sys.exit(128 + os.WTERMSIG(status))
    sys.exit(os.WEXITSTATUS(status))


if __name__ == "__main__":
    main(sys.argv)
