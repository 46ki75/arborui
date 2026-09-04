//! Locates the shared README in both workspace and packaged source layouts.

use std::{env, path::PathBuf};

fn main() {
    let Some(manifest_dir) = env::var_os("CARGO_MANIFEST_DIR") else {
        panic!("Cargo must provide CARGO_MANIFEST_DIR");
    };
    let manifest_dir = PathBuf::from(manifest_dir);
    let packaged_readme = manifest_dir.join("README.md");
    let readme = if packaged_readme.is_file() {
        packaged_readme
    } else {
        manifest_dir.join("../../README.md")
    };
    assert!(readme.is_file(), "arborui README is missing");

    println!("cargo:rerun-if-changed={}", readme.display());
    println!("cargo:rustc-env=ARBORUI_README={}", readme.display());
}
