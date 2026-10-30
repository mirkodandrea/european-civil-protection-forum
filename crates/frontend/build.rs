//! Version stamp shown in the standalone launcher: commit, CI build number, build date.
use std::process::Command;

fn env_or(name: &str, fallback: impl FnOnce() -> String) -> String {
    println!("cargo:rerun-if-env-changed={name}");
    std::env::var(name).ok().filter(|v| !v.is_empty()).unwrap_or_else(fallback)
}

fn command(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn main() {
    let commit = env_or("TTX_BUILD_COMMIT", || {
        let hash = command("git", &["rev-parse", "--short=7", "HEAD"]).unwrap_or_else(|| "unknown".into());
        let dirty = command("git", &["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
        if dirty { format!("{hash}+") } else { hash }
    });
    let number = env_or("TTX_BUILD_NUMBER", || "locale".into());
    let date = env_or("TTX_BUILD_DATE", || command("date", &["-u", "+%Y-%m-%d %H:%M UTC"]).unwrap_or_default());
    println!("cargo:rustc-env=TTX_BUILD_COMMIT={}", commit.chars().take(8).collect::<String>());
    println!("cargo:rustc-env=TTX_BUILD_NUMBER={number}");
    println!("cargo:rustc-env=TTX_BUILD_DATE={date}");
}
