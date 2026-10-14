import { getLocaleFromNavigator, init, register } from 'svelte-i18n';
import { localeCatalog } from './locale-catalog';

export const locales = localeCatalog.map(({ code, name }) => ({ code, name }));

export type SupportedLocale = string;
export const supportedLocales: SupportedLocale[] = locales.map(({ code }) => code);

export function isSupportedLocale(locale: string | null | undefined): locale is SupportedLocale {
  return typeof locale === 'string' && supportedLocales.includes(locale);
}

export function toBcp47Locale(locale: string | null | undefined): string {
  return localeCatalog.find((entry) => entry.code === locale)?.bcp47 ?? 'en-US';
}

const translationLoaders = import.meta.glob('./*.json');

for (const { code, translationSource } of localeCatalog) {
  const loadTranslation = translationLoaders[`./${translationSource}`];
  if (!loadTranslation) throw new Error(`Missing translation source: ${translationSource}`);
  register(code, loadTranslation);
}

const navigatorLocale = getLocaleFromNavigator()?.split(/[-_]/)[0]?.toLowerCase();

init({
  fallbackLocale: 'en',
  initialLocale: isSupportedLocale(navigatorLocale) ? navigatorLocale : 'en',
});
