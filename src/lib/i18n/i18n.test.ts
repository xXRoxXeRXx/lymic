import { describe, expect, it } from 'vitest';
import { isSupportedLocale, locales, supportedLocales } from './index';
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
});
