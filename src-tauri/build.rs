use serde::Deserialize;
use std::{env, fs, path::PathBuf};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LocaleCatalogEntry {
    code: String,
    name: String,
    bcp47: String,
    translation_source: String,
}

fn main() {
    let catalog_path = PathBuf::from("../src/lib/i18n/locale-catalog.json");
    println!("cargo:rerun-if-changed={}", catalog_path.display());

    let catalog: Vec<LocaleCatalogEntry> = serde_json::from_str(
        &fs::read_to_string(&catalog_path).expect("locale catalog must be readable"),
    )
    .expect("locale catalog must be valid JSON");

    let entries = catalog
        .iter()
        .map(|entry| {
            let translation_path = format!("/../src/lib/i18n/{}", entry.translation_source);
            println!(
                "cargo:rerun-if-changed={}",
                catalog_path
                    .parent()
                    .expect("catalog path must have a parent")
                    .join(&entry.translation_source)
                    .display()
            );
            format!(
                "CatalogEntry {{ locale: LocaleCatalogEntry {{ code: {code:?}, name: {name:?}, bcp47: {bcp47:?}, translation_source: {source:?} }}, translations: include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), {translation_path:?})) }}",
                code = entry.code,
                name = entry.name,
                bcp47 = entry.bcp47,
                source = entry.translation_source,
            )
        })
        .collect::<Vec<_>>()
        .join(",\n    ");

    fs::write(
        PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR must be set")).join("locale_catalog.rs"),
        format!("static LOCALE_CATALOG: &[CatalogEntry] = &[\n    {entries}\n];\n"),
    )
    .expect("generated locale catalog must be writable");

    tauri_build::build()
}
