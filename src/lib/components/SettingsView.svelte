<script lang="ts">
  import { Monitor, Moon, Sun, User } from "lucide-svelte";
  import { t, locale } from "svelte-i18n";
  import type { ThemePreference } from "./types";

  interface Props {
    currentUserName: string;
    serverUrl: string;
    isAutostartEnabled: boolean;
    themePreference: ThemePreference;
    uploadParallelism: number;
    isSavingUploadParallelism: boolean;
    onToggleAutostart: () => void;
    onLocaleChange: (locale: string) => void;
    onThemeChange: (preference: ThemePreference) => void;
    onUploadParallelismChange: (event: Event) => void;
    onLogout: () => void;
  }

  let {
    currentUserName,
    serverUrl,
    isAutostartEnabled,
    themePreference,
    uploadParallelism,
    isSavingUploadParallelism,
    onToggleAutostart,
    onLocaleChange,
    onThemeChange,
    onUploadParallelismChange,
    onLogout
  }: Props = $props();
</script>

<div class="space-y-10 animate-in">
  <div class="glass-pane space-y-2 !p-0 overflow-hidden">
    <div class="p-8 flex items-center justify-between border-b border-black/5 dark:border-white/10">
      <div class="space-y-1">
        <p class="font-bold text-slate-900 dark:text-slate-100">{$t('autostart')}</p>
        <p class="text-xs text-slate-500 dark:text-slate-400">{$t('autostart_subtitle')}</p>
      </div>
      <button class="w-12 h-6 border rounded-full relative transition-all {isAutostartEnabled ? 'bg-blue-600 border-blue-600' : 'bg-slate-200 dark:bg-slate-700 border-slate-200 dark:border-slate-700'}" onclick={onToggleAutostart} aria-label={isAutostartEnabled ? $t('autostart_disable') : $t('autostart_enable')} aria-pressed={isAutostartEnabled}>
        <div class="absolute top-1 left-1 w-4 h-4 rounded-full bg-white shadow-sm transition-all {isAutostartEnabled ? 'translate-x-6' : 'translate-x-0'}"></div>
      </button>
    </div>

    <div class="p-8 flex items-center justify-between border-b border-black/5 dark:border-white/10">
      <div class="space-y-1">
        <p class="font-bold text-slate-900 dark:text-slate-100">{$t('language')}</p>
        <p class="text-xs text-slate-500 dark:text-slate-400">{$t('language_subtitle')}</p>
      </div>
      <div class="flex gap-2">
        <button class="px-3 py-1 rounded-lg text-xs font-bold transition-colors {$locale === 'en' ? 'bg-blue-600 text-white' : 'bg-slate-100 dark:bg-slate-800 text-slate-600 dark:text-slate-300 hover:bg-slate-200 dark:hover:bg-slate-700'}" onclick={() => onLocaleChange('en')} aria-pressed={$locale === 'en'}>
          EN
        </button>
        <button class="px-3 py-1 rounded-lg text-xs font-bold transition-colors {$locale === 'de' ? 'bg-blue-600 text-white' : 'bg-slate-100 dark:bg-slate-800 text-slate-600 dark:text-slate-300 hover:bg-slate-200 dark:hover:bg-slate-700'}" onclick={() => onLocaleChange('de')} aria-pressed={$locale === 'de'}>
          DE
        </button>
      </div>
    </div>

    <div class="p-8 flex items-center justify-between gap-6 border-b border-black/5 dark:border-white/10">
      <div class="space-y-1">
        <p class="font-bold text-slate-900 dark:text-slate-100">{$t('appearance')}</p>
        <p class="text-xs text-slate-500 dark:text-slate-400">{$t('appearance_subtitle')}</p>
      </div>
      <div class="flex gap-2" role="group" aria-label={$t('appearance')}>
        <button type="button" class="px-3 py-2 rounded-lg text-xs font-bold transition-colors {themePreference === 'system' ? 'bg-blue-600 text-white' : 'bg-slate-100 dark:bg-slate-800 text-slate-600 dark:text-slate-300 hover:bg-slate-200 dark:hover:bg-slate-700'}" aria-pressed={themePreference === 'system'} title={$t('theme_system')} onclick={() => onThemeChange('system')}>
          <Monitor class="w-4 h-4" />
          <span class="sr-only">{$t('theme_system')}</span>
        </button>
        <button type="button" class="px-3 py-2 rounded-lg text-xs font-bold transition-colors {themePreference === 'light' ? 'bg-blue-600 text-white' : 'bg-slate-100 dark:bg-slate-800 text-slate-600 dark:text-slate-300 hover:bg-slate-200 dark:hover:bg-slate-700'}" aria-pressed={themePreference === 'light'} title={$t('theme_light')} onclick={() => onThemeChange('light')}>
          <Sun class="w-4 h-4" />
          <span class="sr-only">{$t('theme_light')}</span>
        </button>
        <button type="button" class="px-3 py-2 rounded-lg text-xs font-bold transition-colors {themePreference === 'dark' ? 'bg-blue-600 text-white' : 'bg-slate-100 dark:bg-slate-800 text-slate-600 dark:text-slate-300 hover:bg-slate-200 dark:hover:bg-slate-700'}" aria-pressed={themePreference === 'dark'} title={$t('theme_dark')} onclick={() => onThemeChange('dark')}>
          <Moon class="w-4 h-4" />
          <span class="sr-only">{$t('theme_dark')}</span>
        </button>
      </div>
    </div>

    <div class="p-8 flex items-center justify-between gap-6 border-b border-black/5 dark:border-white/10">
      <div class="space-y-1"><label for="upload-parallelism" class="font-bold text-slate-900 dark:text-slate-100">{$t('upload_parallelism')}</label><p class="text-xs text-slate-500 dark:text-slate-400">{$t('upload_parallelism_subtitle')}</p></div>
      <select id="upload-parallelism" class="glass-input !w-auto !py-2 text-sm font-semibold" value={uploadParallelism} onchange={onUploadParallelismChange} disabled={isSavingUploadParallelism}>
        {#each [1, 2, 3, 4, 5, 6, 7, 8] as value}
          <option value={value}>{value}</option>
        {/each}
      </select>
    </div>

    <div class="p-8 flex items-center justify-between">
      <div class="space-y-1">
        <p class="font-bold text-slate-900 dark:text-slate-100">{$t('account')}</p>
        {#if currentUserName}
          <p class="text-sm font-semibold text-slate-700 dark:text-slate-300 flex items-center gap-2"><User class="w-4 h-4 text-slate-400 dark:text-slate-400" />{$t('logged_in_as')} {currentUserName}</p>
        {/if}
        <p class="text-xs text-slate-500 dark:text-slate-400">{$t('logged_in_at')} {serverUrl}</p>
      </div>
      <button class="text-xs font-bold text-red-500 dark:text-red-400 hover:bg-red-50 dark:hover:bg-red-950/50 px-4 py-2 rounded-lg transition-colors" onclick={onLogout}>{$t('logout')}</button>
    </div>
  </div>
</div>
