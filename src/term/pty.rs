//! A pseudo-terminal for the tests. The integration tests cannot see the
//! library's test code, so `tests/it/common` compiles this file too.

use std::ffi::OsStr;
use std::fs::File;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;

use rustix::fs::OFlags;
use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};

/// A pseudo-terminal: the terminal side, and the program's side.
pub fn pty() -> (File, File) {
    let terminal = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).unwrap();
    grantpt(&terminal).unwrap();
    unlockpt(&terminal).unwrap();
    let name = ptsname(&terminal, Vec::new()).unwrap();
    let tty = File::options()
        .read(true)
        .write(true)
        .custom_flags(OFlags::NOCTTY.bits() as i32)
        .open(OsStr::from_bytes(name.as_bytes()))
        .unwrap();
    (File::from(terminal), tty)
}
