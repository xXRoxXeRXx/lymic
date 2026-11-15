<script lang="ts">
  import { UserRound } from "lucide-svelte";
  import { t } from "svelte-i18n";
  import type { RecognizedPerson } from "../contracts";

  let { person, onSelect }: { person: RecognizedPerson; onSelect: (person: RecognizedPerson) => void } = $props();
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

<button type="button" class="rounded-xl border border-slate-200 p-4 text-left dark:border-zinc-700" aria-label={person.name ?? $t("people_unnamed")} onclick={() => onSelect(person)}>
  <div class="mb-3 flex aspect-square items-center justify-center overflow-hidden rounded-lg bg-slate-100 dark:bg-zinc-800">
    {#if person.thumbnailPath && !thumbnailFailed}
      <img class="h-full w-full object-cover" src={thumbnailSrc} alt={person.name ?? $t("people_unnamed")} onerror={() => thumbnailFailed = true} />
    {:else}
      <UserRound class="h-12 w-12 text-slate-400" />
    {/if}
  </div>
  <span class="block truncate font-semibold">{person.name ?? $t("people_unnamed")}</span>
</button>
