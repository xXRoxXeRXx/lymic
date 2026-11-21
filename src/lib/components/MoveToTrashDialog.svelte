<script lang="ts">
  import { Loader2, Trash2 } from "lucide-svelte";
  import { locale, t } from "svelte-i18n";
  import { toBcp47Locale } from "../i18n";

  interface Props {
    isOpen: boolean;
    /** How many Remote Assets will be moved; the message names it. */
    count: number;
    isMoving: boolean;
    onConfirm: () => void;
    onCancel: () => void;
  }

  let { isOpen, count, isMoving, onConfirm, onCancel }: Props = $props();
  const numberLocale = $derived(toBcp47Locale($locale));

  function handleKeydown(event: KeyboardEvent) {
    if (isOpen && event.key === "Escape" && !isMoving) {
      event.preventDefault();
      onCancel();
    }
  }

  function handleBackdropClick(event: MouseEvent) {
    if (event.target === event.currentTarget && !isMoving) onCancel();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if isOpen}
  <div class="fixed inset-0 z-60 flex items-center justify-center bg-black/40 p-4 backdrop-blur-xs" role="presentation" onclick={handleBackdropClick}>
    <!-- svelte-ignore a11y_no_redundant_roles The dialog role is explicit, as in the other dialogs. -->
    <dialog open role="dialog" aria-modal="true" aria-labelledby="move-to-trash-dialog-title" aria-describedby="move-to-trash-dialog-message" class="static m-0 w-full max-w-md space-y-5 rounded-xl border border-slate-200 bg-white p-6 text-slate-900 shadow-xl dark:border-zinc-800 dark:bg-zinc-900 dark:text-slate-100">
      <div class="flex items-start gap-3">
        <div class="shrink-0 rounded-lg bg-red-50 p-2.5 text-red-600 dark:bg-red-950/40 dark:text-red-400"><Trash2 class="h-5 w-5" /></div>
        <div class="min-w-0">
          <h2 id="move-to-trash-dialog-title" class="text-base font-bold">{$t("trash_move_confirm_title")}</h2>
          <p id="move-to-trash-dialog-message" class="mt-1 text-sm text-slate-600 dark:text-zinc-300">{$t("trash_move_confirm_message", { values: { count: count.toLocaleString(numberLocale) } })}</p>
        </div>
      </div>
      <div class="flex items-center justify-end gap-3 border-t border-slate-100 pt-4 dark:border-zinc-800">
        <button type="button" class="rounded-lg px-4 py-2 text-sm font-semibold text-slate-600 hover:bg-slate-100 disabled:opacity-50 dark:text-zinc-300 dark:hover:bg-zinc-800" disabled={isMoving} onclick={onCancel}>{$t("trash_move_cancel")}</button>
        <button type="button" class="flex items-center gap-1.5 rounded-lg bg-red-600 px-4 py-2 text-sm font-semibold text-white shadow-sm hover:bg-red-500 disabled:opacity-50" disabled={isMoving} aria-busy={isMoving} onclick={onConfirm}>{#if isMoving}<Loader2 class="h-4 w-4 animate-spin" />{/if}{$t("trash_move")}</button>
      </div>
    </dialog>
  </div>
{/if}
