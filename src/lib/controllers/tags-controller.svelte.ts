import type { Tag } from "../contracts";
import type { TagsDesktopAdapter } from "../tauri/desktop-adapter";
import { createExploreListController } from "./explore-list-controller.svelte";
import { createRefreshWhenActive } from "./refresh-when-active";
import type { RequestFailure } from "./request-failure";
import { buildTagTree, visibleTagRows, type TagRow } from "./tags-tree";

export interface TagsViewState {
  /** The visible lines of the tree, in order. */
  readonly rows: TagRow[];
  readonly isLoading: boolean;
  readonly isOffline: boolean;
  readonly error: RequestFailure | null;
  /** The ids of every Tag in the last successful load (also while offline, from the cache); null before the first successful load. */
  readonly knownIds: ReadonlySet<string> | null;
  /** True when the Library has no Tags to show. */
  readonly isEmpty: boolean;
  /** Expands or collapses a Tag that has children; kept in memory until `clear`. */
  toggle(tagId: string): void;
  setExpanded(tagId: string, expanded: boolean): void;
  refresh(): Promise<void>;
  /** Reloads at once while the Tags view is open; otherwise remembers that the tree is stale. */
  refreshIfActive(): Promise<void>;
  /** Reloads only when a Synchronization settled while the Tags view was not open. */
  refreshIfStale(): Promise<void>;
  clear(): void;
}

export function createTagsController(adapter: TagsDesktopAdapter, options: { isActive?(): boolean } = {}): TagsViewState {
  const list = createExploreListController<Tag>({ prefix: "tags", load: () => adapter.getTags() });
  // The top level starts expanded and deeper levels collapsed; a choice overrides that default.
  let choices = $state<Record<string, boolean>>({});
  const knownIds = $derived(list.isLoaded ? new Set(list.items.map((tag) => tag.id)) : null);
  const tree = $derived(buildTagTree(list.items));
  const rows = $derived(visibleTagRows(tree, (tag, level) => choices[tag.id] ?? level === 1));
  const refreshing = createRefreshWhenActive({ isActive: () => options.isActive?.() ?? false, refresh: list.refresh });

  function setExpanded(tagId: string, expanded: boolean) {
    choices = { ...choices, [tagId]: expanded };
  }

  return {
    get rows() { return rows; },
    get isLoading() { return list.isLoading; },
    get isOffline() { return list.isOffline; },
    get error() { return list.error; },
    get knownIds() { return knownIds; },
    get isEmpty() { return list.items.length === 0; },
    toggle(tagId) {
      const row = rows.find((candidate) => candidate.tag.id === tagId);
      if (row?.hasChildren) setExpanded(tagId, !row.expanded);
    },
    setExpanded,
    refresh: refreshing.refresh,
    refreshIfActive: refreshing.refreshIfActive,
    refreshIfStale: refreshing.refreshIfStale,
    clear() {
      list.clear();
      choices = {};
    },
  };
}
