use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

pub(crate) struct LocaleState(pub(crate) Mutex<String>);

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocaleCatalogEntry {
    pub(crate) code: &'static str,
    pub(crate) name: &'static str,
    pub(crate) bcp47: &'static str,
    pub(crate) translation_source: &'static str,
}

struct CatalogEntry {
    locale: LocaleCatalogEntry,
    translations: &'static str,
}

#[derive(serde::Deserialize)]
pub(crate) struct BackendTranslations {
    pub(crate) tray_quit: String,
    pub(crate) tray_show: String,
    pub(crate) tray_sync: String,
    pub(crate) tray_pause_sync: String,
    pub(crate) tray_resume_sync: String,
    pub(crate) tray_eta: String,
    pub(crate) tray_idle_status: String,
    pub(crate) notification_complete_title: String,
    pub(crate) notification_complete_body: String,
    #[serde(default)]
    pub(crate) notification_database_repaired_title: Option<String>,
    #[serde(default)]
    pub(crate) log_database_repaired_notice: Option<String>,
}

include!(concat!(env!("OUT_DIR"), "/locale_catalog.rs"));

fn locale_catalog() -> &'static [CatalogEntry] {
    LOCALE_CATALOG
}

pub(crate) fn locale_catalog_entries() -> Vec<LocaleCatalogEntry> {
    locale_catalog()
        .iter()
        .map(|entry| entry.locale.clone())
        .collect()
}

pub(crate) fn is_supported_locale(locale: &str) -> bool {
    locale_catalog()
        .iter()
        .any(|entry| entry.locale.code == locale)
}

pub(crate) fn detect_system_locale() -> &'static str {
    let tag = sys_locale::get_locale().unwrap_or_default();
    let code = tag
        .split(['_', '-', '.'])
        .next()
        .unwrap_or_default()
        .to_lowercase();
    locale_catalog()
        .iter()
        .find(|entry| entry.locale.code == code)
        .map(|entry| entry.locale.code)
        .unwrap_or("en")
}

pub(crate) fn backend_translations(locale: &str) -> &'static BackendTranslations {
    static TRANSLATIONS: OnceLock<HashMap<&'static str, BackendTranslations>> = OnceLock::new();
    let translations = TRANSLATIONS.get_or_init(|| {
        locale_catalog()
            .iter()
            .map(|entry| {
                (
                    entry.locale.code,
                    serde_json::from_str(entry.translations)
                        .expect("catalog translation source must be valid"),
                )
            })
            .collect()
    });

    translations
        .get(locale)
        .or_else(|| translations.get("en"))
        .expect("locale catalog must define English")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_the_shared_locale_catalog_to_the_webview() {
        let catalog = locale_catalog_entries();

        let expected = [
            ("en", "English", "en-US", "en.json"),
            ("de", "Deutsch", "de-DE", "de.json"),
            ("zh", "中文", "zh-CN", "zh.json"),
            ("hi", "हिन्दी", "hi-IN", "hi.json"),
            ("es", "Español", "es-ES", "es.json"),
            ("fr", "Français", "fr-FR", "fr.json"),
            ("ar", "العربية", "ar-SA", "ar.json"),
            ("bn", "বাংলা", "bn-BD", "bn.json"),
            ("pt", "Português", "pt-PT", "pt.json"),
            ("ru", "Русский", "ru-RU", "ru.json"),
        ];

        assert_eq!(catalog.len(), expected.len());
        for (entry, (code, name, bcp47, translation_source)) in catalog.iter().zip(expected) {
            assert_eq!(entry.code, code);
            assert_eq!(entry.name, name);
            assert_eq!(entry.bcp47, bcp47);
            assert_eq!(entry.translation_source, translation_source);
        }
    }
}
