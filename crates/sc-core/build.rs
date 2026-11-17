//! The translations in `l10n/` compiled in: a language added there needs no code.

use std::path::Path;

fn main() {
    let dir = Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../l10n").canonicalize().expect("the l10n folder");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut tags: Vec<String> = std::fs::read_dir(&dir)
        .expect("read l10n")
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter_map(|name| name.strip_suffix(".json").map(str::to_owned))
        .filter(|tag| tag != "locales" && tag != "glossary")
        .collect();
    tags.sort();
    let mut code = String::from("/// Each language's file, by its tag.\npub(crate) const FILES: &[(&str, &str)] = &[\n");
    for tag in &tags {
        let file = dir.join(format!("{tag}.json"));
        println!("cargo:rerun-if-changed={}", file.display());
        code.push_str(&format!("    ({tag:?}, include_str!({:?})),\n", file.display().to_string()));
    }
    code.push_str("];\n");
    code.push_str(&format!("/// The languages' names: `l10n/locales.json`.\npub(crate) const LOCALES: &str = include_str!({:?});\n", dir.join("locales.json").display().to_string()));
    std::fs::write(Path::new(&std::env::var("OUT_DIR").unwrap()).join("l10n.rs"), code).unwrap();
}
