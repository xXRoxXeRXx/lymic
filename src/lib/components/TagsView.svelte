<script lang="ts">
  import { t } from "svelte-i18n";
  import type { Tag } from "../contracts";
  import type { TagsViewState } from "../controllers/tags-controller.svelte";
  import { messageForFailure } from "../controllers/request-failure";
  import TagTree from "./TagTree.svelte";

  /** `onSelect` opens the Tag's Tag Timeline; expanding is the chevron or the arrow keys. */
  let { tags, onSelect }: { tags: TagsViewState; onSelect: (tag: Tag) => void } = $props();
</script>

<div class="flex min-h-0 flex-1 flex-col gap-4">
  {#if tags.isOffline}<p role="status" class="text-sm text-amber-700 dark:text-amber-300">{$t("tags_offline")}</p>{/if}
  {#if tags.isLoading}<p role="status">{$t("tags_loading")}</p>
  {:else if tags.error}
    <div role="alert">
      <p>{messageForFailure(tags.error, { permission_denied: $t("tags_permission_denied"), unavailable: $t("tags_unavailable"), error: $t("tags_error") })}</p>
      <button type="button" onclick={() => void tags.refresh()}>{$t("tags_retry")}</button>
    </div>
  {:else if tags.isEmpty}<p>{$t("tags_empty")}</p>
  {:else}
    <TagTree {tags} {onSelect} />
  {/if}
</div>
