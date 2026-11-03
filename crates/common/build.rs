//! Works out the game's version for [`VERSION`](crate::VERSION):
//! `OURCRAFT_VERSION` if it's set (release builds set it to the tag, like
//! `v0.2.0`), otherwise `git describe` (like `v0.1.2-10-gac539ac`: ten
//! commits after v0.1.2, at commit ac539ac), otherwise Cargo's version.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=OURCRAFT_VERSION");
    let version = std::env::var("OURCRAFT_VERSION")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(git_describe)
        .unwrap_or_else(|| format!("v{}", std::env::var("CARGO_PKG_VERSION").unwrap()));
    println!("cargo:rustc-env=OURCRAFT_VERSION={}", version.trim());
}

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .filter(|s| !s.is_empty())
}

fn git_describe() -> Option<String> {
    // Work it out again after a commit, a checkout or a new tag.
    if let Some(dir) = git(&["rev-parse", "--git-dir"]).map(PathBuf::from) {
        for f in ["HEAD", "packed-refs", "refs/tags"] {
            println!("cargo:rerun-if-changed={}", dir.join(f).display());
        }
        if let Some(head) = git(&["symbolic-ref", "-q", "HEAD"]) {
            println!("cargo:rerun-if-changed={}", dir.join(head).display());
        }
    }
    git(&["describe", "--tags", "--always"])
}
