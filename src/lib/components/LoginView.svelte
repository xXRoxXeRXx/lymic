<script lang="ts">
  import { AlertCircle, ChevronDown, CircleHelp, Eye, EyeOff, Globe, KeyRound, Loader2, X } from "lucide-svelte";
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
  let showApiKey = $state(false);
  let showCloudflareSecret = $state(false);
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
  <div class="text-center space-y-1.5">
    <h1 class="text-2xl font-bold text-slate-900 dark:text-slate-100">{$t('login')}</h1>
    <p class="text-slate-500 dark:text-zinc-400 text-xs">{$t('login_subtitle')}</p>
  </div>
  <div class="desktop-card p-6 space-y-5">
    <div class="space-y-4">
      <!-- Server URL -->
      <div class="space-y-1.5">
        <label for="server-url" class="text-xs font-semibold text-slate-500 dark:text-zinc-400 uppercase tracking-wider">{$t('server_url')}</label>
        <div class="relative flex items-center">
          <Globe class="absolute left-3 w-4 h-4 text-slate-400 dark:text-zinc-500 pointer-events-none" />
          <input
            id="server-url"
            type="text"
            placeholder={$t('server_url_placeholder')}
            class="glass-input !pl-9.5 text-sm"
            bind:value={serverUrl}
          />
        </div>
      </div>

      <!-- API Key -->
      <div class="space-y-1.5">
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-1">
            <label for="api-key" class="text-xs font-semibold text-slate-500 dark:text-zinc-400 uppercase tracking-wider">{$t('api_key')}</label>
            <button
              bind:this={apiKeyHelpButton}
              type="button"
              class="cursor-pointer rounded p-0.5 text-slate-400 hover:bg-slate-100 hover:text-slate-600 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue-600 dark:hover:bg-zinc-800 dark:hover:text-zinc-200 dark:focus-visible:outline-sky-400 transition-colors"
              aria-label={$t('api_key_help_button')}
              onclick={openApiKeyHelp}
            >
              <CircleHelp class="h-3.5 w-3.5" />
            </button>
          </div>
        </div>
        <div class="relative flex items-center">
          <KeyRound class="absolute left-3 w-4 h-4 text-slate-400 dark:text-zinc-500 pointer-events-none" />
          <input
            id="api-key"
            type={showApiKey ? "text" : "password"}
            placeholder={$t('api_key_placeholder')}
            class="glass-input !pl-9.5 !pr-10 text-sm"
            bind:value={apiKey}
          />
          <button
            type="button"
            class="absolute right-2.5 p-1 rounded text-slate-400 hover:text-slate-600 dark:hover:text-zinc-200 transition-colors cursor-pointer"
            onclick={() => showApiKey = !showApiKey}
            aria-label={showApiKey ? "Hide API key" : "Show API key"}
            tabindex="-1"
          >
            {#if showApiKey}
              <EyeOff class="w-4 h-4" />
            {:else}
              <Eye class="w-4 h-4" />
            {/if}
          </button>
        </div>
      </div>

      <!-- Advanced options -->
      <details class="group rounded-lg border border-black/5 dark:border-white/5 bg-slate-50/50 dark:bg-zinc-800/20 p-3 transition-colors space-y-3" bind:open={advancedDetailsOpen}>
        <summary class="flex items-center justify-between cursor-pointer list-none text-xs font-semibold text-slate-600 dark:text-zinc-300 select-none [&::-webkit-details-marker]:hidden">
          <span>{$t('advanced_login_options')}</span>
          <ChevronDown class="w-4 h-4 text-slate-400 dark:text-zinc-500 transition-transform duration-200 group-open:rotate-180" />
        </summary>
        <div class="space-y-4 pt-3 border-t border-slate-200/60 dark:border-zinc-800">
          <section class="space-y-2" aria-labelledby="local-http-options">
            <h2 id="local-http-options" class="text-xs font-semibold text-slate-700 dark:text-slate-200">{$t('local_http_options')}</h2>
            <div class="flex items-start gap-2.5 text-xs text-slate-600 dark:text-slate-300">
              <input
                id="allow-local-http"
                type="checkbox"
                class="mt-0.5 rounded cursor-pointer accent-blue-600"
                aria-describedby="allow-local-http-description"
                bind:checked={allowLocalHttp}
              />
              <span>
                <label for="allow-local-http" class="font-medium cursor-pointer">{$t('allow_local_http')}</label>
                <span id="allow-local-http-description" class="block text-[11px] text-slate-400 dark:text-zinc-500 mt-0.5 leading-normal">{$t('allow_local_http_description')}</span>
              </span>
            </div>
          </section>
          <section class="space-y-3 pt-3 border-t border-slate-200/60 dark:border-zinc-800" aria-labelledby="cloudflare-access-options">
            <div class="space-y-0.5">
              <h2 id="cloudflare-access-options" class="text-xs font-semibold text-slate-700 dark:text-slate-200">{$t('cloudflare_access_options')}</h2>
              <p class="text-[11px] text-slate-400 dark:text-zinc-500">{$t('cloudflare_access_description')}</p>
            </div>

            <div class="space-y-1.5">
              <label for="cloudflare-client-id" class="text-[11px] font-semibold text-slate-500 dark:text-zinc-400 uppercase tracking-wider">{$t('cloudflare_client_id')}</label>
              <input id="cloudflare-client-id" type="password" placeholder={$t('cloudflare_client_id_placeholder')} class="glass-input !py-2 text-xs" bind:value={cloudflareClientId} />
            </div>
            <div class="space-y-1.5">
              <label for="cloudflare-client-secret" class="text-[11px] font-semibold text-slate-500 dark:text-zinc-400 uppercase tracking-wider">{$t('cloudflare_client_secret')}</label>
              <div class="relative flex items-center">
                <input
                  id="cloudflare-client-secret"
                  type={showCloudflareSecret ? "text" : "password"}
                  placeholder={$t('cloudflare_client_secret_placeholder')}
                  class="glass-input !py-2 !pr-9 text-xs"
                  bind:value={cloudflareClientSecret}
                />
                <button
                  type="button"
                  class="absolute right-2.5 p-1 rounded text-slate-400 hover:text-slate-600 dark:hover:text-zinc-200 transition-colors cursor-pointer"
                  onclick={() => showCloudflareSecret = !showCloudflareSecret}
                  aria-label={showCloudflareSecret ? "Hide secret" : "Show secret"}
                  tabindex="-1"
                >
                  {#if showCloudflareSecret}
                    <EyeOff class="w-3.5 h-3.5" />
                  {:else}
                    <Eye class="w-3.5 h-3.5" />
                  {/if}
                </button>
              </div>
            </div>
          </section>
        </div>
      </details>
    </div>
    {#if loginError}
      <div class="p-3 bg-red-50/70 dark:bg-red-950/30 border border-red-200/80 dark:border-red-900/50 text-red-600 dark:text-red-300 text-xs rounded-lg flex items-center gap-2.5">
        <AlertCircle class="w-4 h-4 shrink-0" />
        <span>{loginError}</span>
      </div>
    {/if}
    <button
      class="w-full py-2.5 rounded-lg bg-blue-600 hover:bg-blue-700 disabled:opacity-50 text-white font-semibold text-sm shadow-sm transition-colors cursor-pointer flex items-center justify-center gap-2"
      onclick={onLogin}
      disabled={isLoggingIn || !serverUrl || !apiKey}
    >
      {#if isLoggingIn}
        <Loader2 class="w-4 h-4 animate-spin shrink-0" />
      {/if}
      <span>{isLoggingIn ? $t('connect') : $t('login')}</span>
    </button>
  </div>
</div>

{#if isApiKeyHelpOpen}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-xs p-4 animate-in" role="presentation" onclick={handleApiKeyHelpBackdropClick}>
    <dialog open class="static m-0 w-full max-w-lg rounded-xl border border-slate-200 dark:border-zinc-800 bg-white p-6 shadow-xl dark:bg-zinc-900 text-slate-900 dark:text-slate-100 space-y-4" aria-labelledby="api-key-help-title" aria-modal="true">
      <div class="flex items-start justify-between gap-4">
        <div class="flex items-center gap-3 min-w-0">
          <div class="p-2.5 rounded-lg bg-blue-50 dark:bg-blue-950/40 text-blue-600 dark:text-sky-400 shrink-0">
            <CircleHelp class="w-5 h-5" />
          </div>
          <h2 id="api-key-help-title" class="text-base font-bold text-slate-900 dark:text-slate-100 truncate">{$t('api_key_help_title')}</h2>
        </div>
        <button type="button" class="rounded-lg p-1.5 text-slate-400 hover:bg-slate-100 hover:text-slate-600 dark:hover:bg-zinc-800 dark:hover:text-zinc-200 transition-colors cursor-pointer shrink-0" aria-label={$t('api_key_help_close')} onclick={closeApiKeyHelp}>
          <X class="h-4 w-4" />
        </button>
      </div>
      <p class="mt-1 text-xs text-slate-600 dark:text-zinc-400 leading-relaxed">{$t('api_key_help_creation')}</p>
      <h3 class="mt-3 text-xs font-bold text-slate-900 dark:text-slate-100 uppercase tracking-wider">{$t('api_key_help_permissions_title')}</h3>
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
