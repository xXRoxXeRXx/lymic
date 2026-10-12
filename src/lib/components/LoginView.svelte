<script lang="ts">
  import { AlertCircle } from "lucide-svelte";
  import { t } from "svelte-i18n";

  let {
    serverUrl = $bindable(),
    apiKey = $bindable(),
    allowLocalHttp = $bindable(false),
    cloudflareClientId = $bindable(""),
    cloudflareClientSecret = $bindable(""),
    isLoggingIn,
    loginError,
    onLogin
  }: {
    serverUrl: string;
    apiKey: string;
    allowLocalHttp: boolean;
    cloudflareClientId: string;
    cloudflareClientSecret: string;
    isLoggingIn: boolean;
    loginError: string;
    onLogin: () => void;
  } = $props();

  let cloudflareDetailsOpen = $state(false);

  $effect(() => {
    if (
      cloudflareClientId.trim() ||
      cloudflareClientSecret.trim() ||
      loginError === $t('cloudflare_access_pair_required') ||
      loginError === $t('local_http_not_allowed')
    ) {
      cloudflareDetailsOpen = true;
    }
  });
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
      <details class="space-y-4" bind:open={cloudflareDetailsOpen}>
        <summary class="cursor-pointer text-sm font-semibold text-slate-600 dark:text-slate-300">{$t('advanced_login_options')}</summary>
        <p class="text-xs text-slate-500 dark:text-slate-400">{$t('cloudflare_access_description')}</p>
        <div class="flex items-start gap-3 text-sm text-slate-600 dark:text-slate-300">
          <input id="allow-local-http" type="checkbox" class="mt-0.5" bind:checked={allowLocalHttp} />
          <span>
            <label for="allow-local-http" class="font-semibold">{$t('allow_local_http')}</label>
            <span class="block text-xs text-slate-500 dark:text-slate-400">{$t('allow_local_http_description')}</span>
          </span>
        </div>
        <div class="space-y-2">
          <label for="cloudflare-client-id" class="text-xs font-bold text-slate-400 dark:text-slate-400 uppercase tracking-wider ml-1">{$t('cloudflare_client_id')}</label>
          <input id="cloudflare-client-id" type="password" placeholder={$t('cloudflare_client_id_placeholder')} class="glass-input" bind:value={cloudflareClientId} />
        </div>
        <div class="space-y-2">
          <label for="cloudflare-client-secret" class="text-xs font-bold text-slate-400 dark:text-slate-400 uppercase tracking-wider ml-1">{$t('cloudflare_client_secret')}</label>
          <input id="cloudflare-client-secret" type="password" placeholder={$t('cloudflare_client_secret_placeholder')} class="glass-input" bind:value={cloudflareClientSecret} />
        </div>
      </details>
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
