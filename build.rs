//! Tells the code which version of merman `Cargo.lock` has, since Cargo
//! tells a crate only its own version.

fn main() {
    println!("cargo:rerun-if-changed=Cargo.lock");
    let lock = std::fs::read_to_string("Cargo.lock").unwrap_or_default();
    let version = lock
        .split("[[package]]")
        .find(|entry| entry.contains("name = \"merman\"\n"))
        .and_then(|entry| entry.lines().find_map(|l| l.strip_prefix("version = \"")))
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or("unknown");
    println!("cargo:rustc-env=MERMAN_VERSION={version}");
}
