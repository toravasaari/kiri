//! No global mutable state (AGENTS.md, "Architecture rules"), as a test.
//!
//! The guard reads the non-test code of `src/` and of each workspace crate's
//! `src/`, and finds:
//!
//! - every `static mut`;
//! - every `static` inside `thread_local!` or `lazy_static!`, by its own name,
//!   so that an allowed one does not let a second in the same block pass;
//! - every other `static` whose type names a type with inner mutability
//!   (`Cell`, `RefCell`, `OnceCell`, `UnsafeCell`, `LazyCell`, `Mutex`,
//!   `RwLock`, an `Atomic…`) or `mut`. The names are matched whole: kiri's
//!   own `CellText` or `Cells` are no `Cell`.
//!
//! `OnceLock` and `LazyLock` of a value without inner mutability are set
//! once and then only read, and tables of constants are not state; both
//! pass. What stays is in `ALLOWED`, with why, and that list can only
//! shrink: an entry the code no longer holds fails too. A new entry is the
//! maintainer's decision.
//!
//! What it does not see: a `static` that a macro other than those two
//! makes; state behind an `Arc<Mutex<…>>` that a function hands to
//! everybody, which is no `static`; a type of kiri's own that holds a
//! `Mutex` inside; and the state the process has outside the code's
//! statics, such as the panic hook (`src/term/screen.rs`) and the signal
//! handlers (`src/term/interrupt.rs`), which are set on purpose in the
//! terminal layer.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::rust_source::{Strings, is_ident, item_end, non_test_sources};

/// The global mutable state that stays, as (file, name, why).
const ALLOWED: &[(&str, &str, &str)] = &[(
    "src/panics.rs",
    "CATCHING",
    "the panic hook is the process's own, and it asks whether the panic on its \
     thread is one that `panics::catch` catches: a flag of the thread is the one \
     place it can ask",
)];

/// The types whose values can change behind a shared reference.
const INNER_MUTABILITY: &[&str] = &[
    "Cell",
    "RefCell",
    "OnceCell",
    "UnsafeCell",
    "LazyCell",
    "Mutex",
    "RwLock",
];

#[test]
fn no_code_keeps_global_mutable_state() {
    let mut found = BTreeSet::new();
    for src in source_roots() {
        for source in non_test_sources(&src, Strings::Blank) {
            for (line, name) in globals(&source.code) {
                found.insert((source.shown.clone(), line, name));
            }
        }
    }
    let allowed: BTreeSet<(&str, &str)> = ALLOWED
        .iter()
        .map(|&(file, name, _)| (file, name))
        .collect();
    let new: Vec<String> = found
        .iter()
        .filter(|(file, _, name)| !allowed.contains(&(file.as_str(), name.as_str())))
        .map(|(file, line, name)| format!("  {file}:{line}: {name}"))
        .collect();
    let gone: Vec<String> = allowed
        .iter()
        .filter(|&&(file, name)| !found.iter().any(|(f, _, n)| f == file && n == name))
        .map(|(file, name)| format!("  {file}: {name}"))
        .collect();
    assert!(
        new.is_empty(),
        "code keeps global mutable state (AGENTS.md, \"Architecture rules\"):\n{}\n\
         Pass the state to what needs it instead. If it has to stay, adding it to \
         ALLOWED with why is the maintainer's decision.\n",
        new.join("\n")
    );
    assert!(
        gone.is_empty(),
        "listed in ALLOWED but no longer in the code:\n{}\n\
         Remove those entries: the list can only shrink.\n",
        gone.join("\n")
    );
}

/// `src/` and the `src/` of each workspace crate.
fn source_roots() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut roots = vec![root.join("src")];
    let mut crates: Vec<PathBuf> = fs::read_dir(root.join("crates"))
        .unwrap()
        .map(|entry| entry.unwrap().path().join("src"))
        .filter(|src| src.is_dir())
        .collect();
    assert!(!crates.is_empty(), "no workspace crates under crates/");
    crates.sort();
    roots.extend(crates);
    roots
}

/// The global mutable state in `code`, which has no comments, strings or
/// test code, as (line number, name).
fn globals(code: &str) -> Vec<(usize, String)> {
    let blocks: Vec<(usize, usize)> = ["thread_local!", "lazy_static!"]
        .iter()
        .flat_map(|name| words(code, name).map(|at| (at, item_end(code, at + name.len()))))
        .collect();
    let mut found = Vec::new();
    for at in words(code, "static") {
        if code[..at].ends_with('\'') {
            continue; // A lifetime: `&'static str`.
        }
        let rest = code[at + "static".len()..].trim_start();
        let (mutable, rest) = match rest.strip_prefix("mut") {
            Some(after) if !after.starts_with(is_ident) => (true, after.trim_start()),
            _ => (false, rest),
        };
        let name: String = rest.chars().take_while(|&c| is_ident(c)).collect();
        if name.is_empty() {
            continue;
        }
        let in_block = blocks
            .iter()
            .any(|&(start, end)| (start..end).contains(&at));
        let ty = rest[name.len()..]
            .trim_start()
            .strip_prefix(':')
            .map(type_text);
        if mutable || in_block || ty.is_some_and(changes) {
            found.push((code[..at].matches('\n').count() + 1, name));
        }
    }
    found
}

/// Where the word `word` starts in `code`, as a whole word.
fn words<'a>(code: &'a str, word: &'a str) -> impl Iterator<Item = usize> + 'a {
    code.match_indices(word)
        .map(|(at, _)| at)
        .filter(move |&at| {
            let before = code[..at].chars().next_back();
            let after = code[at + word.len()..].chars().next();
            before.is_none_or(|c| !is_ident(c))
                && after.is_none_or(|c| !is_ident(c) || word.ends_with('!'))
        })
}

/// The type after a `static`'s colon: up to its `=` or, with no value, its
/// `;`, outside brackets (`[u8; 4]`). An `=` of `==`, `=>`, `<=`, `>=` or
/// `!=` is inside the type.
fn type_text(after_colon: &str) -> &str {
    let bytes = after_colon.as_bytes();
    let mut depth = 0i32;
    let mut end = bytes.len();
    for (i, &byte) in bytes.iter().enumerate() {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b';' if depth <= 0 => {
                end = i;
                break;
            }
            b'=' if depth <= 0 => {
                let next = bytes.get(i + 1).copied();
                let before = i.checked_sub(1).map(|j| bytes[j]);
                if !matches!(next, Some(b'=' | b'>'))
                    && !matches!(before, Some(b'=' | b'<' | b'>' | b'!'))
                {
                    end = i;
                    break;
                }
            }
            _ => {}
        }
    }
    &after_colon[..end]
}

/// Whether a type lets its value change behind a shared reference.
fn changes(ty: &str) -> bool {
    ty.split(|c: char| !is_ident(c))
        .any(|name| name == "mut" || name.starts_with("Atomic") || INNER_MUTABILITY.contains(&name))
}

#[test]
fn the_guard_finds_mutable_statics_and_lets_constant_ones_pass() {
    let names =
        |code: &str| -> Vec<String> { globals(code).into_iter().map(|(_, name)| name).collect() };
    assert_eq!(names("static A: AtomicUsize = AtomicUsize::new(0);"), ["A"]);
    assert_eq!(
        names("pub(crate) static B: Mutex<Vec<u8>> = Mutex::new(Vec::new());"),
        ["B"]
    );
    assert_eq!(names("static mut C: u8 = 0;"), ["C"]);
    assert_eq!(
        names("static L: OnceLock<Mutex<u8>> = OnceLock::new();"),
        ["L"]
    );
    assert_eq!(names("extern \"C\" { static E: AtomicU8; }"), ["E"]);
    assert_eq!(names("static P: *mut u8 = std::ptr::null_mut();"), ["P"]);
    // Multiline, as `src/typeset/driver.rs` has one.
    assert_eq!(
        names("static BUDGET: std::cell::Cell<Option<Duration>> =\n    const { Cell::new(None) };"),
        ["BUDGET"]
    );
    // Each static of a thread-local block, by its own name, whatever its type.
    assert_eq!(
        names("thread_local! {\n    static A: u8 = 0;\n    static B: Cell<u8> = Cell::new(0);\n}"),
        ["A", "B"]
    );
    assert!(names("static TABLE: &[(&str, &str)] = &[];").is_empty());
    assert!(names("static S: OnceLock<SyntaxSet> = OnceLock::new();").is_empty());
    assert!(names("static CELLS: [CellText; 2] = [CellText::A, CellText::B];").is_empty());
    assert!(
        names("static N: [Cells; 1] = [Cells::new()]; static M: MutexGuardLike = X;").is_empty()
    );
    assert!(names("fn f() -> &'static str { \"\" } fn g<T: 'static>() {}").is_empty());
    assert!(names("static Q: Wrap<{ A >= B }> = Wrap;").is_empty());
}

#[test]
fn a_line_number_is_the_statics_own() {
    assert_eq!(globals("\n\nstatic mut X: u8 = 0;"), [(3, "X".to_owned())]);
}
