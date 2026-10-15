<script lang="ts">
  import { AlertCircle, CircleHelp, X } from "lucide-svelte";
  import { tick } from "svelte";
  import { t } from "svelte-i18n";

  let {
    serverUrl = $bindable(),
    apiKey = $bindable(),
    allowLocalHttp = $bindable(false),
    cloudflareClientId = $bindable(""),
    cloudflareClientSecret = $bindable(""),
    isLoggingIn,
    loginError,
    loginErrorCode = "",
    onLogin
  }: {
    serverUrl: string;
    apiKey: string;
    allowLocalHttp: boolean;
    cloudflareClientId: string;
    cloudflareClientSecret: string;
    isLoggingIn: boolean;
    loginError: string;
    loginErrorCode?: string;
    onLogin: () => void;
  } = $props();

  let advancedDetailsOpen = $state(false);
  let isApiKeyHelpOpen = $state(false);
  let apiKeyHelpButton: HTMLButtonElement;

  function openApiKeyHelp() {
    isApiKeyHelpOpen = true;
  }

  async function closeApiKeyHelp() {
    isApiKeyHelpOpen = false;
    await tick();
    apiKeyHelpButton.focus();
  }

  function handleApiKeyHelpKeydown(event: KeyboardEvent) {
    if (isApiKeyHelpOpen && event.key === "Escape") {
      closeApiKeyHelp();
    }
  }

  function handleApiKeyHelpBackdropClick(event: MouseEvent) {
    if (event.target === event.currentTarget) {
      closeApiKeyHelp();
    }
  }

  $effect(() => {
    if (
      cloudflareClientId.trim() ||
      cloudflareClientSecret.trim() ||
      loginErrorCode === 'CLOUDFLARE_ACCESS_PAIR_REQUIRED' ||
      loginErrorCode === 'LOCAL_HTTP_NOT_ALLOWED'
    ) {
      advancedDetailsOpen = true;
    }
  });
</script>

<svelte:window onkeydown={handleApiKeyHelpKeydown} />

<div class="max-w-md w-full mx-auto space-y-6 animate-in">
  <div class="text-center space-y-2">
    <h1 class="text-2xl font-bold text-slate-900 dark:text-slate-100">{$t('login')}</h1>
    <p class="text-slate-500 dark:text-slate-400 text-xs">{$t('login_subtitle')}</p>
  </div>
  <div class="desktop-card p-6 space-y-5">
    <div class="space-y-4">
      <div class="space-y-1.5">
        <label for="server-url" class="text-xs font-semibold text-slate-500 dark:text-zinc-400 uppercase tracking-wider">{$t('server_url')}</label>
        <input id="server-url" type="text" placeholder={$t('server_url_placeholder')} class="glass-input" bind:value={serverUrl} />
      </div>
      <div class="space-y-1.5">
        <div class="flex items-center gap-1">
          <label for="api-key" class="text-xs font-semibold text-slate-500 dark:text-zinc-400 uppercase tracking-wider">{$t('api_key')}</label>
          <button bind:this={apiKeyHelpButton} type="button" class="cursor-pointer rounded p-0.5 text-slate-400 hover:bg-slate-100 hover:text-slate-600 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue-600 dark:hover:bg-slate-800 dark:hover:text-slate-200 dark:focus-visible:outline-blue-400" aria-label={$t('api_key_help_button')} onclick={openApiKeyHelp}>
            <CircleHelp class="h-3.5 w-3.5" />
          </button>
        </div>
        <input id="api-key" type="password" placeholder={$t('api_key_placeholder')} class="glass-input" bind:value={apiKey} />
      </div>
      <details class="space-y-3 pt-1" bind:open={advancedDetailsOpen}>
        <summary class="cursor-pointer text-xs font-semibold text-slate-600 dark:text-zinc-300">{$t('advanced_login_options')}</summary>
        <section class="space-y-2 pt-2 border-t border-slate-100 dark:border-zinc-800" aria-labelledby="local-http-options">
          <h2 id="local-http-options" class="text-xs font-semibold text-slate-700 dark:text-slate-200">{$t('local_http_options')}</h2>
          <div class="flex items-start gap-2.5 text-xs text-slate-600 dark:text-slate-300">
            <input id="allow-local-http" type="checkbox" class="mt-0.5 rounded cursor-pointer" aria-describedby="allow-local-http-description" bind:checked={allowLocalHttp} />
            <span>
              <label for="allow-local-http" class="font-medium cursor-pointer">{$t('allow_local_http')}</label>
              <span id="allow-local-http-description" class="block text-[11px] text-slate-400 dark:text-zinc-500 mt-0.5">{$t('allow_local_http_description')}</span>
            </span>
          </div>
        </section>
        <section class="space-y-2 pt-2 border-t border-slate-100 dark:border-zinc-800" aria-labelledby="cloudflare-access-options">
          <h2 id="cloudflare-access-options" class="text-xs font-semibold text-slate-700 dark:text-slate-200">{$t('cloudflare_access_options')}</h2>
          <p class="text-[11px] text-slate-400 dark:text-zinc-500">{$t('cloudflare_access_description')}</p>

          <div class="space-y-1.5">
            <label for="cloudflare-client-id" class="text-[11px] font-semibold text-slate-500 dark:text-zinc-400 uppercase tracking-wider">{$t('cloudflare_client_id')}</label>
            <input id="cloudflare-client-id" type="password" placeholder={$t('cloudflare_client_id_placeholder')} class="glass-input" bind:value={cloudflareClientId} />
          </div>
          <div class="space-y-1.5">
            <label for="cloudflare-client-secret" class="text-[11px] font-semibold text-slate-500 dark:text-zinc-400 uppercase tracking-wider">{$t('cloudflare_client_secret')}</label>
            <input id="cloudflare-client-secret" type="password" placeholder={$t('cloudflare_client_secret_placeholder')} class="glass-input" bind:value={cloudflareClientSecret} />
          </div>
        </section>
      </details>
    </div>
    {#if loginError}
      <div class="p-3 bg-red-50 dark:bg-red-950/50 text-red-600 dark:text-red-300 text-xs rounded-lg flex items-center gap-2.5">
        <AlertCircle class="w-4 h-4 shrink-0" />
        <span>{loginError}</span>
      </div>
    {/if}
    <button class="w-full py-2.5 rounded-lg bg-blue-600 hover:bg-blue-700 disabled:opacity-50 text-white font-semibold text-sm shadow-sm transition-colors cursor-pointer" onclick={onLogin} disabled={isLoggingIn || !serverUrl || !apiKey}>
      {isLoggingIn ? $t('connect') : $t('login')}
    </button>
  </div>
</div>

{#if isApiKeyHelpOpen}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4" role="presentation" onclick={handleApiKeyHelpBackdropClick}>
    <dialog open class="static m-0 w-full max-w-lg rounded-xl border border-slate-200 dark:border-zinc-800 bg-white p-6 shadow-xl dark:bg-zinc-900" aria-labelledby="api-key-help-title" aria-modal="true">
      <div class="flex items-start justify-between gap-4">
        <h2 id="api-key-help-title" class="text-base font-bold text-slate-900 dark:text-slate-100">{$t('api_key_help_title')}</h2>
        <button type="button" class="rounded-lg p-1 text-slate-500 hover:bg-slate-100 dark:hover:bg-zinc-800 cursor-pointer" aria-label={$t('api_key_help_close')} onclick={closeApiKeyHelp}>
          <X class="h-4 w-4" />
        </button>
      </div>
      <p class="mt-3 text-xs text-slate-600 dark:text-zinc-400 leading-relaxed">{$t('api_key_help_creation')}</p>
      <h3 class="mt-4 text-xs font-bold text-slate-900 dark:text-slate-100 uppercase tracking-wider">{$t('api_key_help_permissions_title')}</h3>
      <ul class="mt-2 space-y-1.5 text-xs text-slate-600 dark:text-zinc-400">
        <li><code class="font-semibold text-slate-800 dark:text-slate-200 bg-slate-100 dark:bg-zinc-800 px-1 py-0.5 rounded text-[11px]">asset.upload</code>: {$t('api_key_help_permission_upload')}</li>
        <li><code class="font-semibold text-slate-800 dark:text-slate-200 bg-slate-100 dark:bg-zinc-800 px-1 py-0.5 rounded text-[11px]">asset.read</code>: {$t('api_key_help_permission_read')}</li>
        <li><code class="font-semibold text-slate-800 dark:text-slate-200 bg-slate-100 dark:bg-zinc-800 px-1 py-0.5 rounded text-[11px]">album.read</code>: {$t('api_key_help_permission_album_read')}</li>
        <li><code class="font-semibold text-slate-800 dark:text-slate-200 bg-slate-100 dark:bg-zinc-800 px-1 py-0.5 rounded text-[11px]">album.create</code>: {$t('api_key_help_permission_album_create')}</li>
        <li><code class="font-semibold text-slate-800 dark:text-slate-200 bg-slate-100 dark:bg-zinc-800 px-1 py-0.5 rounded text-[11px]">albumAsset.create</code>: {$t('api_key_help_permission_album_update')}</li>
        <li><code class="font-semibold text-slate-800 dark:text-slate-200 bg-slate-100 dark:bg-zinc-800 px-1 py-0.5 rounded text-[11px]">user.read</code>: {$t('api_key_help_permission_user_read')}</li>
      </ul>
    </dialog>
  </div>
{/if}
