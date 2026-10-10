use std::sync::{Mutex, OnceLock};

pub(crate) struct LocaleState(pub(crate) Mutex<String>);

#[derive(serde::Deserialize)]
pub(crate) struct BackendTranslations {
    pub(crate) tray_quit: String,
    pub(crate) tray_show: String,
    pub(crate) tray_sync: String,
    pub(crate) tray_transferred: String,
    pub(crate) tray_rate: String,
    pub(crate) tray_eta: String,
    pub(crate) notification_complete_title: String,
    pub(crate) notification_complete_body: String,
}

pub(crate) fn is_supported_locale(locale: &str) -> bool {
    matches!(locale, "en" | "de")
}

// The backend consumes its strings from the frontend's translation catalog so the
// tray and notifications cannot drift from the application language.
pub(crate) fn backend_translations(locale: &str) -> &'static BackendTranslations {
    static EN_TRANSLATIONS: OnceLock<BackendTranslations> = OnceLock::new();
    static DE_TRANSLATIONS: OnceLock<BackendTranslations> = OnceLock::new();

    match locale {
        "de" => DE_TRANSLATIONS.get_or_init(|| {
            serde_json::from_str(include_str!("../../../src/lib/i18n/de.json"))
                .expect("German translations must be valid")
        }),
        _ => EN_TRANSLATIONS.get_or_init(|| {
            serde_json::from_str(include_str!("../../../src/lib/i18n/en.json"))
                .expect("English translations must be valid")
        }),
    }
}
