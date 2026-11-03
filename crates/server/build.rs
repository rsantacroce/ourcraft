//! Bundle the mods in `mods/dist/` into the server (`*.wasm`, and EELisp
//! scripts, `*.el`), so the game has them even where you can't easily copy
//! files (phones and tablets). Run `mods/build.sh` to (re)build them. Also
//! bundles the EELisp runner, `mods/runtimes/eelisp.wasm`, which runs the
//! scripts.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let dist = manifest.join("../../mods/dist");
    println!("cargo:rerun-if-changed={}", dist.display());

    let mut entries = Vec::new();
    let mut scripts = Vec::new();
    if let Ok(dir) = fs::read_dir(&dist) {
        for entry in dir.flatten() {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str());
            if !matches!(ext, Some("wasm" | "el")) {
                continue;
            }
            println!("cargo:rerun-if-changed={}", path.display());
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            let abs = path.canonicalize().unwrap().display().to_string();
            if ext == Some("wasm") {
                entries.push(format!("    ({name:?}, include_bytes!({abs:?})),"));
            } else {
                scripts.push(format!("    ({name:?}, include_str!({abs:?})),"));
            }
        }
    }
    entries.sort();
    scripts.sort();
    let runner = manifest.join("../../mods/runtimes/eelisp.wasm");
    println!("cargo:rerun-if-changed={}", runner.display());
    let runner = match runner.canonicalize() {
        Ok(p) => format!("Some(include_bytes!({:?}))", p.display().to_string()),
        Err(_) => "None".into(),
    };
    let code = format!(
        "/// Mods built into the game: (name, wasm bytes).\npub static BUNDLED_MODS: &[(&str, &[u8])] = &[\n{}\n];\n\
         /// EELisp mods built into the game: (name, script).\npub static BUNDLED_SCRIPTS: &[(&str, &str)] = &[\n{}\n];\n\
         /// The EELisp runner (`mods/runtimes/eelisp.wasm`), if it's been built.\npub static EELISP_RUNNER: Option<&[u8]> = {runner};\n",
        entries.join("\n"),
        scripts.join("\n")
    );
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("bundled_mods.rs");
    fs::write(out, code).unwrap();
}
