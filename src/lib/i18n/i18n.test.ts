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
    const keys = ['memories_title', 'memories_loading', 'memories_offline', 'memories_error', 'memories_permission_denied', 'memory_year_ago', 'memory_years_ago', 'memory_birthday', 'memory_untitled', 'memories_timeline_empty'];
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

  it('uses consistent timeline and video terminology in favorites strings', () => {
    expect(de.favorites_offline).toContain('Zeitachse');
    expect(de.favorites_offline).not.toContain('Timeline');
    expect(es.favorites_empty).toContain('vídeos');
    expect(es.favorites_offline).toContain('línea de tiempo');
    expect(pt.favorites_offline).toContain('linha do tempo');
    expect(ar.favorites_offline).toContain('الخط الزمني');
    expect(ru.favorites_offline).toContain('Хронология');
  });
});
