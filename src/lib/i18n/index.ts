import { register, init, getLocaleFromNavigator } from 'svelte-i18n';

export const locales = [
  { code: 'en', name: 'English' },
  { code: 'de', name: 'Deutsch' },
] as const;

export type SupportedLocale = (typeof locales)[number]['code'];
export const supportedLocales: SupportedLocale[] = locales.map(({ code }) => code);

export function isSupportedLocale(locale: string | null | undefined): locale is SupportedLocale {
  return typeof locale === 'string' && supportedLocales.includes(locale as SupportedLocale);
}

register('en', () => import('./en.json'));
register('de', () => import('./de.json'));

const navigatorLocale = getLocaleFromNavigator()?.split(/[-_]/)[0]?.toLowerCase();

init({
  fallbackLocale: 'en',
  initialLocale: isSupportedLocale(navigatorLocale) ? navigatorLocale : 'en',
});
