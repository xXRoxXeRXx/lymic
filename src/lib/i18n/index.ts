import { register, init, getLocaleFromNavigator } from 'svelte-i18n';

register('en', () => import('./en.json'));
register('de', () => import('./de.json'));

init({
  fallbackLocale: 'en',
  initialLocale: getLocaleFromNavigator()?.split('-')[0] ?? 'en',
});
