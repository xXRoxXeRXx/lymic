use std::sync::{Mutex, OnceLock};

pub(crate) struct LocaleState(pub(crate) Mutex<String>);

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

const SUPPORTED_LOCALES: [&str; 10] = ["en", "de", "zh", "hi", "es", "fr", "ar", "bn", "pt", "ru"];

pub(crate) fn is_supported_locale(locale: &str) -> bool {
    SUPPORTED_LOCALES.contains(&locale)
}

pub(crate) fn detect_system_locale() -> &'static str {
    let tag = sys_locale::get_locale().unwrap_or_default();
    let code = tag
        .split(|c: char| c == '_' || c == '-' || c == '.')
        .next()
        .unwrap_or_default()
        .to_lowercase();
    SUPPORTED_LOCALES
        .iter()
        .copied()
        .find(|supported| *supported == code)
        .unwrap_or("en")
}
pub(crate) fn backend_translations(locale: &str) -> &'static BackendTranslations {
    static EN_TRANSLATIONS: OnceLock<BackendTranslations> = OnceLock::new();
    static DE_TRANSLATIONS: OnceLock<BackendTranslations> = OnceLock::new();
    static ZH_TRANSLATIONS: OnceLock<BackendTranslations> = OnceLock::new();
    static HI_TRANSLATIONS: OnceLock<BackendTranslations> = OnceLock::new();
    static ES_TRANSLATIONS: OnceLock<BackendTranslations> = OnceLock::new();
    static FR_TRANSLATIONS: OnceLock<BackendTranslations> = OnceLock::new();
    static AR_TRANSLATIONS: OnceLock<BackendTranslations> = OnceLock::new();
    static BN_TRANSLATIONS: OnceLock<BackendTranslations> = OnceLock::new();
    static PT_TRANSLATIONS: OnceLock<BackendTranslations> = OnceLock::new();
    static RU_TRANSLATIONS: OnceLock<BackendTranslations> = OnceLock::new();

    match locale {
        "de" => DE_TRANSLATIONS.get_or_init(|| {
            serde_json::from_str(include_str!("../../../src/lib/i18n/de.json"))
                .expect("German translations must be valid")
        }),
        "zh" => ZH_TRANSLATIONS.get_or_init(|| {
            serde_json::from_str(include_str!("../../../src/lib/i18n/zh.json"))
                .expect("Mandarin Chinese translations must be valid")
        }),
        "hi" => HI_TRANSLATIONS.get_or_init(|| {
            serde_json::from_str(include_str!("../../../src/lib/i18n/hi.json"))
                .expect("Hindi translations must be valid")
        }),
        "es" => ES_TRANSLATIONS.get_or_init(|| {
            serde_json::from_str(include_str!("../../../src/lib/i18n/es.json"))
                .expect("Spanish translations must be valid")
        }),
        "fr" => FR_TRANSLATIONS.get_or_init(|| {
            serde_json::from_str(include_str!("../../../src/lib/i18n/fr.json"))
                .expect("French translations must be valid")
        }),
        "ar" => AR_TRANSLATIONS.get_or_init(|| {
            serde_json::from_str(include_str!("../../../src/lib/i18n/ar.json"))
                .expect("Arabic translations must be valid")
        }),
        "bn" => BN_TRANSLATIONS.get_or_init(|| {
            serde_json::from_str(include_str!("../../../src/lib/i18n/bn.json"))
                .expect("Bengali translations must be valid")
        }),
        "pt" => PT_TRANSLATIONS.get_or_init(|| {
            serde_json::from_str(include_str!("../../../src/lib/i18n/pt.json"))
                .expect("Portuguese translations must be valid")
        }),
        "ru" => RU_TRANSLATIONS.get_or_init(|| {
            serde_json::from_str(include_str!("../../../src/lib/i18n/ru.json"))
                .expect("Russian translations must be valid")
        }),
        _ => EN_TRANSLATIONS.get_or_init(|| {
            serde_json::from_str(include_str!("../../../src/lib/i18n/en.json"))
                .expect("English translations must be valid")
        }),
    }
}
