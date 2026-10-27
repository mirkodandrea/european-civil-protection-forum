use std::path::Path;
use std::process::Command;

fn main() {
    let manifest =
        Path::new(&std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR")).to_path_buf();
    let root = manifest.join("../..");

    // Rebuild the served WASM bundle when its source, its local Rust dependency,
    // or the build configuration changes. Cargo caches this build script, so a
    // normal `cargo run` does not invoke Trunk again when these inputs are stable.
    for path in [
        "crates/frontend/src",
        "crates/frontend/assets",
        "crates/frontend/index.html",
        "crates/frontend/Cargo.toml",
        "crates/frontend/Trunk.toml",
        "crates/scenario/src",
        "crates/scenario/Cargo.toml",
        "scripts/build_frontend.sh",
        "Cargo.lock",
    ] {
        println!("cargo:rerun-if-changed={}", root.join(path).display());
    }

    let status = Command::new("sh")
        .arg(root.join("scripts/build_frontend.sh"))
        .current_dir(&root)
        // The outer Cargo process holds a lock for its own target directory.
        // Trunk runs Cargo recursively, so give that build a separate target
        // directory to avoid waiting on the parent process forever.
        .env("CARGO_TARGET_DIR", root.join("target/frontend"))
        .status()
        .expect(
            "could not start frontend build; install Trunk and the wasm32-unknown-unknown target",
        );
    assert!(
        status.success(),
        "frontend build failed; see Trunk output above"
    );
}
