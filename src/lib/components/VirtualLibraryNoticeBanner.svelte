<script lang="ts">
  import { AlertCircle, X } from "lucide-svelte";
  import { t } from "svelte-i18n";

  interface Props {
    /** The backend's one notice for a failed Virtual Library read; `key` is its locale key, taking the item's `name`. */
    notice: { name: string; key: string; canRelogin: boolean };
    onRelogin: () => void;
    onDismiss: () => void;
  }

  let { notice, onRelogin, onDismiss }: Props = $props();
</script>

<div class="p-4 bg-amber-50/70 dark:bg-amber-950/30 border border-amber-200/80 dark:border-amber-900/50 text-amber-800 dark:text-amber-200 text-xs rounded-xl flex items-center gap-3" role="alert">
  <AlertCircle class="w-4 h-4 shrink-0" />
  <span class="flex-1 break-words">{$t(notice.key, { values: { name: notice.name } })}</span>
  {#if notice.canRelogin}
    <button
      type="button"
      class="shrink-0 px-3 py-1.5 rounded-lg bg-amber-600 hover:bg-amber-700 text-white font-semibold transition-colors cursor-pointer"
      onclick={onRelogin}
    >{$t("virtual_library_notice_relogin")}</button>
  {/if}
  <button
    type="button"
    class="p-1 rounded text-amber-500 hover:text-amber-700 hover:bg-amber-100 dark:text-amber-300 dark:hover:text-amber-100 dark:hover:bg-amber-900/50 transition-colors cursor-pointer shrink-0"
    aria-label={$t("virtual_library_notice_dismiss")}
    onclick={onDismiss}
  >
    <X class="w-4 h-4" />
  </button>
</div>
