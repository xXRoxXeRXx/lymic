<script lang="ts">
  import { Info, ExternalLink } from "lucide-svelte";
  import { t } from "svelte-i18n";
  import logo from "#lib/assets/logo.png";

  interface Props {
    appVersion?: string | null;
  }

  let { appVersion }: Props = $props();
</script>

<div class="max-w-2xl w-full space-y-6 animate-in pb-10">
  <!-- Brand / Header Card -->
  <div class="settings-card flex items-center gap-4 !p-5">
    <img src={logo} alt="Lymic Logo" class="w-12 h-12 object-contain shrink-0" />
    <div class="space-y-0.5">
      <h2 class="text-base font-bold text-slate-900 dark:text-slate-100">Lymic</h2>
      <p class="text-xs text-slate-500 dark:text-slate-400">Unofficial Immich Desktop Client</p>
    </div>
  </div>

  <!-- Information Section -->
  <section class="space-y-3">
    <h3 class="text-xs font-bold uppercase tracking-wider text-slate-400 dark:text-zinc-500 flex items-center gap-2 px-1">
      <Info class="w-3.5 h-3.5" />
      <span>{$t('info')}</span>
    </h3>

    <div class="desktop-card divide-y divide-black/[0.04] dark:divide-white/[0.04] overflow-hidden">
      <!-- Name -->
      <div class="flex items-center justify-between p-4 gap-4">
        <span class="text-xs font-semibold text-slate-500 dark:text-slate-400 shrink-0">{$t('app_name_label')}</span>
        <span class="text-xs font-medium text-slate-900 dark:text-slate-100 text-right">
          Lymic (Unofficial Immich Desktop Client)
        </span>
      </div>

      <!-- Version -->
      <div class="flex items-center justify-between p-4 gap-4">
        <span class="text-xs font-semibold text-slate-500 dark:text-slate-400 shrink-0">{$t('version_label')}</span>
        <span class="text-xs font-medium font-mono text-slate-900 dark:text-slate-100">
          {appVersion ? `v${appVersion}` : "-"}
        </span>
      </div>

      <!-- License -->
      <div class="flex items-center justify-between p-4 gap-4">
        <span class="text-xs font-semibold text-slate-500 dark:text-slate-400 shrink-0">{$t('license_label')}</span>
        <span class="text-xs font-medium text-slate-900 dark:text-slate-100">
          MIT
        </span>
      </div>

      <!-- Source Code -->
      <div class="flex items-center justify-between p-4 gap-4">
        <span class="text-xs font-semibold text-slate-500 dark:text-slate-400 shrink-0">{$t('source_code_label')}</span>
        <a
          href="https://github.com/xXRoxXeRXx/lymic/"
          target="_blank"
          rel="noopener noreferrer"
          class="text-xs font-medium text-blue-600 dark:text-sky-400 hover:underline flex items-center gap-1.5 truncate"
        >
          <span class="truncate">https://github.com/xXRoxXeRXx/lymic/</span>
          <ExternalLink class="w-3 h-3 shrink-0" />
        </a>
      </div>
    </div>
  </section>
</div>
