<script lang="ts">
  import { UserRound } from "lucide-svelte";
  import { t } from "svelte-i18n";
  import type { RecognizedPerson } from "../contracts";

  /** `selected` makes the tile a toggle (for choosing people); `compact` is the smaller tile of a picker. */
  let { person, onSelect, selected, compact = false }: { person: RecognizedPerson; onSelect: (person: RecognizedPerson) => void; selected?: boolean; compact?: boolean } = $props();
  let thumbnailFailed = $state(false);

  const thumbnailSrc = $derived.by(() => {
    const encodedId = encodeURIComponent(person.id);
    const version = encodeURIComponent(person.thumbnailPath ?? "");
    const convertFileSrc = (
      globalThis as typeof globalThis & {
        __TAURI_INTERNALS__?: { convertFileSrc?: (filePath: string, protocol?: string) => string };
      }
    ).__TAURI_INTERNALS__?.convertFileSrc;
    if (typeof convertFileSrc === "function") {
      return `${convertFileSrc("", "lymic-thumb").replace(/\/$/, "")}/person/${encodedId}?v=${version}`;
    }
    return `lymic-thumb://person/${encodedId}?v=${version}`;
  });
</script>

<button type="button" class="rounded-xl border text-left {compact ? 'p-2 text-sm' : 'p-4'} {selected ? 'border-blue-600 bg-blue-50 ring-2 ring-blue-600 dark:border-sky-400 dark:bg-sky-950/40 dark:ring-sky-400' : 'border-slate-200 dark:border-zinc-700'}" aria-pressed={selected} aria-label={person.name ?? $t("people_unnamed")} onclick={() => onSelect(person)}>
  <div class="{compact ? 'mb-2' : 'mb-3'} flex aspect-square items-center justify-center overflow-hidden rounded-lg bg-slate-100 dark:bg-zinc-800">
    {#if person.thumbnailPath && !thumbnailFailed}
      <img class="h-full w-full object-cover" src={thumbnailSrc} alt={person.name ?? $t("people_unnamed")} onerror={() => thumbnailFailed = true} />
    {:else}
      <UserRound class="h-12 w-12 text-slate-400" />
    {/if}
  </div>
  <span class="block truncate font-semibold">{person.name ?? $t("people_unnamed")}</span>
</button>
