<script lang="ts">
  import { AlertCircle } from "lucide-svelte";
  import { t } from "svelte-i18n";

  let {
    serverUrl = $bindable(),
    apiKey = $bindable(),
    isLoggingIn,
    loginError,
    onLogin
  }: {
    serverUrl: string;
    apiKey: string;
    isLoggingIn: boolean;
    loginError: string;
    onLogin: () => void;
  } = $props();
</script>

<div class="max-w-md w-full mx-auto space-y-10 animate-in">
  <div class="text-center space-y-4">
    <h1 class="text-3xl font-bold text-slate-900 dark:text-slate-100">{$t('login')}</h1>
    <p class="text-slate-500 dark:text-slate-400 text-sm">{$t('login_subtitle')}</p>
  </div>
  <div class="glass-pane space-y-8">
    <div class="space-y-6">
      <div class="space-y-2">
        <label for="server-url" class="text-xs font-bold text-slate-400 dark:text-slate-400 uppercase tracking-wider ml-1">{$t('server_url')}</label>
        <input id="server-url" type="text" placeholder={$t('server_url_placeholder')} class="glass-input" bind:value={serverUrl} />
      </div>
      <div class="space-y-2">
        <label for="api-key" class="text-xs font-bold text-slate-400 dark:text-slate-400 uppercase tracking-wider ml-1">{$t('api_key')}</label>
        <input id="api-key" type="password" placeholder={$t('api_key_placeholder')} class="glass-input" bind:value={apiKey} />
      </div>
    </div>
    {#if loginError}
      <div class="p-4 bg-red-50 dark:bg-red-950/50 text-red-600 dark:text-red-300 text-xs rounded-xl flex items-center gap-3">
        <AlertCircle class="w-4 h-4" />
        <span>{loginError}</span>
      </div>
    {/if}
    <button class="btn-action w-full" onclick={onLogin} disabled={isLoggingIn || !serverUrl || !apiKey}>
      {isLoggingIn ? $t('connect') : $t('login')}
    </button>
  </div>
</div>
