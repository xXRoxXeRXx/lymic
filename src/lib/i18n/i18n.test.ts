import { describe, expect, it } from 'vitest';
import { locale } from 'svelte-i18n';
import { localeCatalog } from './locale-catalog';
import { isSupportedLocale, locales, supportedLocales, toBcp47Locale } from './index';
import de from './de.json';
import en from './en.json';
import zh from './zh.json';
import hi from './hi.json';
import es from './es.json';
import fr from './fr.json';
import ar from './ar.json';
import bn from './bn.json';
import pt from './pt.json';
import ru from './ru.json';

const catalog = {
  de,
  en,
  zh,
  hi,
  es,
  fr,
  ar,
  bn,
  pt,
  ru,
} as const;

describe('i18n catalogs', () => {
  it('does not contain replacement question marks in map translations', () => {
    for (const [localeCode, translations] of Object.entries(catalog)) {
      for (const [key, value] of Object.entries(translations)) {
        if (key.startsWith('map_')) expect(value, `${localeCode}.${key}`).not.toContain('?');
      }
    }
  });
  it('translates the Explore Overview strings in every locale', () => {
    const keys = ['explore_title', 'explore_view_all', 'explore_retry', 'explore_empty', 'explore_return'];
    for (const [localeCode, translations] of Object.entries(catalog)) {
      for (const key of keys) {
        expect(translations, `Locale ${localeCode} is missing ${key}`).toHaveProperty(key);
        expect((translations as Record<string, string>)[key].trim(), `${localeCode}.${key}`).not.toBe('');
      }
    }
    expect(de.explore_title).toBe('Erkunden');
  });

  it('translates the Explore Places strings in every locale', () => {
    const keys = ['places_title', 'places_all_title', 'places_loading', 'places_empty', 'places_offline', 'places_error', 'places_unavailable', 'places_permission_denied'];
    for (const [localeCode, translations] of Object.entries(catalog)) {
      for (const key of keys) {
        expect(translations, `Locale ${localeCode} is missing ${key}`).toHaveProperty(key);
        expect((translations as Record<string, string>)[key].trim(), `${localeCode}.${key}`).not.toBe('');
      }
      expect((translations as Record<string, string>).places_permission_denied, localeCode).toContain('asset.read');
    }
  });

  it('translates the Place Timeline strings in every locale', () => {
    for (const [localeCode, translations] of Object.entries(catalog)) {
      for (const key of ['places_return', 'places_timeline_empty', 'places_timeline_error']) {
        expect(translations, `Locale ${localeCode} is missing ${key}`).toHaveProperty(key);
        expect((translations as Record<string, string>)[key].trim(), `${localeCode}.${key}`).not.toBe('');
      }
    }
  });

  it('translates the Memories strings in every locale', () => {
    const keys = ['memories_title', 'memories_loading', 'memories_offline', 'memories_error', 'memories_unavailable', 'memories_permission_denied', 'memory_year_ago', 'memory_years_ago', 'memory_birthday', 'memory_untitled', 'memories_timeline_empty'];
    for (const [localeCode, translations] of Object.entries(catalog)) {
      for (const key of keys) {
        expect(translations, `Locale ${localeCode} is missing ${key}`).toHaveProperty(key);
        expect((translations as Record<string, string>)[key].trim(), `${localeCode}.${key}`).not.toBe('');
      }
      expect((translations as Record<string, string>).memories_permission_denied, localeCode).toContain('memory.read');
      expect((translations as Record<string, string>).memory_birthday, localeCode).toContain('{name}');
    }
    expect(de.memories_title).toBe('Erinnerungen');
  });

  it('names the person.read permission in the People permission message of every locale', () => {
    for (const [localeCode, translations] of Object.entries(catalog)) {
      const message = (translations as Record<string, string>).people_permission_denied;
      expect(message?.trim(), localeCode).toBeTruthy();
      expect(message, localeCode).toContain('person.read');
    }
  });

  it('translates the All Memories strings in every locale', () => {
    const keys = ['memories_all_title', 'memories_all_empty', 'memories_loading_more', 'memories_more_error', 'memories_all_return'];
    for (const [localeCode, translations] of Object.entries(catalog)) {
      for (const key of keys) {
        expect(translations, `Locale ${localeCode} is missing ${key}`).toHaveProperty(key);
        expect((translations as Record<string, string>)[key].trim(), `${localeCode}.${key}`).not.toBe('');
      }
    }
    expect(de.memories_all_title).toBe('Alle Erinnerungen');
  });

  it('updates the document language when the active locale changes', async () => {
    await locale.set('de');
    expect(document.documentElement.lang).toBe('de-DE');

    await locale.set('en');
    expect(document.documentElement.lang).toBe('en-US');

    expect(toBcp47Locale(null)).toBe('en-US');
  });

  it('translates every backend log message emitted to the UI', () => {
    const backendLogKeys = {
      log_duplicate_resolve_failed: ['{file}', '{error}'],
      log_upload_completion_persist_failed: ['{file}', '{error}'],
      log_removable_volume_error: ['{error}'],
    };

    for (const [localeCode, translations] of Object.entries(catalog)) {
      for (const [key, placeholders] of Object.entries(backendLogKeys)) {
        expect(translations, `Locale ${localeCode} is missing ${key}`).toHaveProperty(key);
        const translation = (translations as Record<string, string>)[key];
        for (const placeholder of placeholders) {
          expect(translation, `Locale ${localeCode} is missing ${placeholder} in ${key}`).toContain(placeholder);
        }
      }
    }
  });

  it('defines each locale once with its display name, BCP-47 tag, and translation source', () => {
    expect(localeCatalog).toEqual([
      { code: 'en', name: 'English', bcp47: 'en-US', translationSource: 'en.json' },
      { code: 'de', name: 'Deutsch', bcp47: 'de-DE', translationSource: 'de.json' },
      { code: 'zh', name: '中文', bcp47: 'zh-CN', translationSource: 'zh.json' },
      { code: 'hi', name: 'हिन्दी', bcp47: 'hi-IN', translationSource: 'hi.json' },
      { code: 'es', name: 'Español', bcp47: 'es-ES', translationSource: 'es.json' },
      { code: 'fr', name: 'Français', bcp47: 'fr-FR', translationSource: 'fr.json' },
      { code: 'ar', name: 'العربية', bcp47: 'ar-SA', translationSource: 'ar.json' },
      { code: 'bn', name: 'বাংলা', bcp47: 'bn-BD', translationSource: 'bn.json' },
      { code: 'pt', name: 'Português', bcp47: 'pt-PT', translationSource: 'pt.json' },
      { code: 'ru', name: 'Русский', bcp47: 'ru-RU', translationSource: 'ru.json' },
    ]);
  });

  it('registers all 10 supported locales', () => {
    const expected = ['en', 'de', 'zh', 'hi', 'es', 'fr', 'ar', 'bn', 'pt', 'ru'];
    expect(supportedLocales).toEqual(expected);
    expect(locales.map((l) => l.code)).toEqual(expected);

    for (const code of expected) {
      expect(isSupportedLocale(code)).toBe(true);
    }
    expect(isSupportedLocale('it')).toBe(false);
    expect(isSupportedLocale('')).toBe(false);
    expect(isSupportedLocale(null)).toBe(false);
    expect(isSupportedLocale(undefined)).toBe(false);
  });

  it('ensures all translation catalogs match de.json keys and placeholders', () => {
    const baseKeys = Object.keys(de).sort();
    const placeholderRegex = /\{([^}]+)\}/g;

    for (const [localeCode, translations] of Object.entries(catalog)) {
      const keys = Object.keys(translations).sort();
      expect(keys, `Locale ${localeCode} missing or surplus keys`).toEqual(baseKeys);

      for (const key of baseKeys) {
        const baseValue = (de as Record<string, string>)[key];
        const val = (translations as Record<string, string>)[key];
        expect(val, `Locale ${localeCode} has empty key ${key}`).toBeTruthy();

        // Extract placeholders like {version}, {processed}, etc.
        const basePlaceholders = (baseValue.match(placeholderRegex) || []).sort();
        const valuePlaceholders = (val.match(placeholderRegex) || []).sort();
        expect(
          valuePlaceholders,
          `Placeholders mismatch in locale ${localeCode} for key ${key}`
        ).toEqual(basePlaceholders);
      }
    }
  });

  it('translates the Archive Timeline strings in every locale', () => {
    const keys = ['archive_title', 'archive_loading', 'archive_empty', 'archive_error', 'archive_offline', 'archive_permission_error'];
    for (const [localeCode, translations] of Object.entries(catalog)) {
      for (const key of keys) {
        expect(translations, `Locale ${localeCode} is missing ${key}`).toHaveProperty(key);
        expect((translations as Record<string, string>)[key].trim(), `${localeCode}.${key}`).not.toBe('');
      }
    }
    expect(en.archive_title).toBe('Archive');
    expect(en.archive_empty).toBe('Your archive is empty.');
    expect(de.archive_title).toBe('Archiv');
    for (const [localeCode, translations] of Object.entries(catalog)) {
      expect((translations as Record<string, string>).archive_permission_error, `${localeCode} names the permission`).toContain('asset.read');
    }
  });

  it('translates the Trash Timeline strings in every locale', () => {
    const keys = ['trash_title', 'trash_loading', 'trash_empty', 'trash_error', 'trash_offline', 'trash_hint'];
    for (const [localeCode, translations] of Object.entries(catalog)) {
      for (const key of keys) {
        expect(translations, `Locale ${localeCode} is missing ${key}`).toHaveProperty(key);
        expect((translations as Record<string, string>)[key].trim(), `${localeCode}.${key}`).not.toBe('');
      }
      // The retention hint is fixed text: the server's retention period is not known to Lymic.
      expect((translations as Record<string, string>).trash_hint, `${localeCode}.trash_hint has no number`).not.toMatch(/\d/);
    }
    expect(en.trash_title).toBe('Trash');
    expect(en.trash_empty).toBe('Your trash is empty.');
    expect(de.trash_title).toBe('Papierkorb');
  });

  it('translates the Trash Timeline Restore strings in every locale with their placeholders', () => {
    const placeholders: Record<string, string[]> = {
      trash_restore: [],
      trash_restore_success: ['{count}'],
      trash_restore_partial: ['{failed}', '{count}'],
      trash_restore_failed: [],
      trash_restore_none: ['{failed}', '{count}'],
      trash_restore_permission: [],
    };
    for (const [localeCode, translations] of Object.entries(catalog)) {
      for (const [key, expected] of Object.entries(placeholders)) {
        const value = (translations as Record<string, string>)[key];
        expect(value, `Locale ${localeCode} is missing ${key}`).toBeTruthy();
        for (const placeholder of expected) expect(value, `${localeCode}.${key}`).toContain(placeholder);
      }
      // A refused Restore names the permission to grant, never a credential.
      expect((translations as Record<string, string>).trash_restore_permission, localeCode).toContain('asset.delete');
    }
    expect(en.trash_restore).toBe('Restore');
    expect(de.trash_restore).toBe('Wiederherstellen');
  });

  it('uses consistent timeline and video terminology in favorites strings', () => {
    expect(de.favorites_offline).toContain('Zeitachse');
    expect(de.favorites_offline).not.toContain('Timeline');
    expect(es.favorites_empty).toContain('vídeos');
    expect(es.favorites_offline).toContain('línea de tiempo');
    expect(pt.favorites_offline).toContain('linha do tempo');
    expect(ar.favorites_offline).toContain('الخط الزمني');
    expect(ru.favorites_offline).toContain('Хронология');
  });

  it('translates the Search strings in every locale with their placeholders and permission names', () => {
    const placeholders: Record<string, string[]> = {
      search_title: [],
      search_label: [],
      search_placeholder: [],
      search_submit: [],
      search_clear: [],
      search_return: ['{view}'],
      search_loading: [],
      search_empty: [],
      search_results_count: ['{count}'],
      search_truncated: ['{count}'],
      search_unsupported: ['{required}', '{version}'],
      search_authentication_required: [],
      search_permission_denied: [],
      search_unavailable: [],
      search_invalid_response: [],
      search_offline: [],
    };
    for (const [localeCode, translations] of Object.entries(catalog)) {
      for (const [key, expected] of Object.entries(placeholders)) {
        const value = (translations as Record<string, string>)[key];
        expect(value, `Locale ${localeCode} is missing ${key}`).toBeTruthy();
        for (const placeholder of expected) expect(value, `${localeCode}.${key}`).toContain(placeholder);
      }
      // A refused Search names the permission to grant, never a credential.
      expect((translations as Record<string, string>).search_permission_denied, localeCode).toContain('asset.read');
    }
    expect(en.search_title).toBe('Search');
    expect(de.search_title).toBe('Suche');
  });

  it('translates the Search filter panel strings in every locale', () => {
    const keys = [
      'search_filter_date',
      'search_filter_date_from',
      'search_filter_date_to',
      'search_filter_date_clear',
      'search_filter_date_invalid_range',
      'search_filter_media_type',
      'search_filter_media_all',
      'search_filter_media_image',
      'search_filter_media_video',
    ];
    for (const [localeCode, translations] of Object.entries(catalog)) {
      for (const key of keys) {
        expect((translations as Record<string, string>)[key], `Locale ${localeCode} is missing ${key}`).toBeTruthy();
      }
    }
    expect(en.search_filter_media_video).toBe('Videos');
    expect(de.search_filter_media_type).toBe('Medientyp');
  });

  it('translates the camera filter and suggestion strings in every locale', () => {
    const keys = [
      'search_filter_camera', 'search_filter_camera_make', 'search_filter_camera_model', 'search_filter_camera_lens',
      'search_suggestions_loading', 'search_suggestions_empty', 'search_suggestions_no_match', 'search_suggestions_failed',
      'search_suggestions_permission_denied', 'search_suggestions_retry', 'search_suggestions_clear',
    ];
    for (const [localeCode, translations] of Object.entries(catalog)) {
      for (const key of keys) {
        expect((translations as Record<string, string>)[key], `Locale ${localeCode} is missing ${key}`).toBeTruthy();
      }
      // A missing suggestion permission names the permission to grant; clearing names the filter.
      expect((translations as Record<string, string>).search_suggestions_permission_denied, localeCode).toContain('asset.read');
      expect((translations as Record<string, string>).search_suggestions_clear, localeCode).toContain('{filter}');
    }
    expect(en.search_filter_camera_lens).toBe('Lens model');
    expect(de.search_filter_camera_make).toBe('Kamerahersteller');
  });

  it('translates the Context Search strings in every locale', () => {
    const keys = [
      'search_mode_context', 'search_placeholder_context', 'search_mode_context_unavailable',
      'search_context_unavailable', 'search_more_error', 'search_loading_more',
    ];
    for (const [localeCode, translations] of Object.entries(catalog)) {
      for (const key of keys) {
        expect((translations as Record<string, string>)[key], `Locale ${localeCode} is missing ${key}`).toBeTruthy();
      }
    }
    expect(en.search_mode_context).toBe('Context');
    expect(de.search_mode_context).toBe('Kontext');
  });

  it('translates the Search Mode strings in every locale', () => {
    const keys = [
      'search_options', 'search_mode_label', 'search_mode_filename', 'search_mode_description', 'search_mode_folderPath', 'search_mode_ocr',
      'search_mode_ocr_unavailable', 'search_features_unavailable', 'search_placeholder_description', 'search_placeholder_folderPath', 'search_placeholder_ocr',
    ];
    for (const [localeCode, translations] of Object.entries(catalog)) {
      for (const key of keys) {
        expect((translations as Record<string, string>)[key], `Locale ${localeCode} is missing ${key}`).toBeTruthy();
      }
    }
    expect(en.search_mode_folderPath).toBe('Folder Path');
    expect(de.search_mode_filename).toBe('Dateiname');
    expect(de.search_mode_folderPath).toBe('Ordnerpfad');
  });
});
