import { register, init, getLocaleFromNavigator } from 'svelte-i18n';

export const supportedLocales = ['en', 'de'] as const;
export type SupportedLocale = (typeof supportedLocales)[number];

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
