import { register, init, getLocaleFromNavigator } from 'svelte-i18n';

export const locales = [
  { code: 'en', name: 'English' },
  { code: 'de', name: 'Deutsch' },
  { code: 'zh', name: '中文' },
  { code: 'hi', name: 'हिन्दी' },
  { code: 'es', name: 'Español' },
  { code: 'fr', name: 'Français' },
  { code: 'ar', name: 'العربية' },
  { code: 'bn', name: 'বাংলা' },
  { code: 'pt', name: 'Português' },
  { code: 'ru', name: 'Русский' },
] as const;

export type SupportedLocale = (typeof locales)[number]['code'];
export const supportedLocales: SupportedLocale[] = locales.map(({ code }) => code);

export function isSupportedLocale(locale: string | null | undefined): locale is SupportedLocale {
  return typeof locale === 'string' && supportedLocales.includes(locale as SupportedLocale);
}

register('en', () => import('./en.json'));
register('de', () => import('./de.json'));
register('zh', () => import('./zh.json'));
register('hi', () => import('./hi.json'));
register('es', () => import('./es.json'));
register('fr', () => import('./fr.json'));
register('ar', () => import('./ar.json'));
register('bn', () => import('./bn.json'));
register('pt', () => import('./pt.json'));
register('ru', () => import('./ru.json'));

const navigatorLocale = getLocaleFromNavigator()?.split(/[-_]/)[0]?.toLowerCase();

init({
  fallbackLocale: 'en',
  initialLocale: isSupportedLocale(navigatorLocale) ? navigatorLocale : 'en',
});
