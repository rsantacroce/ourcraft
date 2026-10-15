//! Bundle the mods in `mods/dist/*.wasm` into the server, so the game has
//! them even where you can't easily copy files (phones and tablets).
//! Run `mods/build.sh` to (re)build them.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let dist = manifest.join("../../mods/dist");
    println!("cargo:rerun-if-changed={}", dist.display());

    let mut entries = Vec::new();
    if let Ok(dir) = fs::read_dir(&dist) {
        for entry in dir.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "wasm") {
                println!("cargo:rerun-if-changed={}", path.display());
                let name = path.file_stem().unwrap().to_string_lossy().into_owned();
                let abs = path.canonicalize().unwrap();
                entries.push(format!(
                    "    ({name:?}, include_bytes!({:?})),",
                    abs.display().to_string()
                ));
            }
        }
    }
    entries.sort();
    let code = format!(
        "/// Mods built into the game: (name, wasm bytes).\npub static BUNDLED_MODS: &[(&str, &[u8])] = &[\n{}\n];\n",
        entries.join("\n")
    );
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("bundled_mods.rs");
    fs::write(out, code).unwrap();
}
