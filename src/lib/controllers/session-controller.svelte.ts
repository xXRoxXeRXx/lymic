import { createAlbumCatalogueController } from "./album-catalogue-controller.svelte";
import { createAuthController } from "./auth-controller.svelte";
import { createExploreController } from "./explore-controller.svelte";
import { createFilteredTimelineControllers } from "./filtered-timeline-controller.svelte";
import { createFeedbackController } from "./feedback-controller.svelte";
import { createAllMemoriesController } from "./all-memories-controller.svelte";
import { createMemoriesController } from "./memories-controller.svelte";
import { createMapController } from "./map-controller.svelte";
import { createPeopleController } from "./people-controller.svelte";
import { createPlacesController } from "./places-controller.svelte";
import { createRemovableVolumeController } from "./removable-volume-controller.svelte";
import { createSearchController } from "./search-controller.svelte";
import { createSyncController } from "./sync-controller.svelte";
import { createTagsController } from "./tags-controller.svelte";
import { createTargetAlbumCatalogue } from "./target-album-catalogue.svelte";
import { createTimelineController } from "./timeline-controller.svelte";
import { createWatchedFoldersController } from "./watched-folders-controller.svelte";
import type {
  AlbumCatalogueDesktopAdapter,
  AllMemoriesDesktopAdapter,
  AlbumsDesktopAdapter,
  AuthDesktopAdapter,
  FilteredTimelineDesktopAdapter,
  MapDesktopAdapter,
  MemoriesDesktopAdapter,
  PeopleDesktopAdapter,
  PlacesDesktopAdapter,
  RemovableVolumeDesktopAdapter,
  SearchDesktopAdapter,
  SyncDesktopAdapter,
  TagsDesktopAdapter,
  TimelineDesktopAdapter,
  WatchedFoldersDesktopAdapter,
} from "../tauri/desktop-adapter";

export type SessionDesktopAdapter =
  AuthDesktopAdapter &
  SyncDesktopAdapter &
  WatchedFoldersDesktopAdapter &
  AlbumsDesktopAdapter &
  AlbumCatalogueDesktopAdapter &
  TimelineDesktopAdapter &
  FilteredTimelineDesktopAdapter &
  MapDesktopAdapter &
  PeopleDesktopAdapter &
  PlacesDesktopAdapter &
  TagsDesktopAdapter &
  MemoriesDesktopAdapter &
  AllMemoriesDesktopAdapter &
  SearchDesktopAdapter &
  RemovableVolumeDesktopAdapter;

export function createSessionController(options: {
  adapter: SessionDesktopAdapter;
  t(key: string, params?: Record<string, string>): string;
  locale?(): string;
  resetView?(): void;
  /** Whether the Explore Overview is the open view; it refreshes after Synchronization settles only then. */
  isExploreActive?(): boolean;
  /** Whether the Tags view is the open view; it reloads after Synchronization settles only then. */
  isTagsActive?(): boolean;
  /** Whether the Virtual Library is on; logging out then names its cleanup in the confirmation. */
  isVirtualLibraryEnabled?(): boolean;
}) {
  const { adapter, t } = options;
  const feedback = createFeedbackController(() => t("unknown_error"));
  const targetAlbumCatalogue = createTargetAlbumCatalogue(adapter);
  const removable = createRemovableVolumeController({
    adapter,
    t,
    onVolumeRemounted: () => void watchedFolders.refresh(),
  });
  const watchedFolders = createWatchedFoldersController({
    adapter,
    reportError: feedback.report,
    t: (key) => t(key),
    targetAlbumCatalogue,
    onRemovableVolumeAccepted: (volumeId) => removable.dismiss(volumeId),
    onFolderRemoved: () => sync.refreshFailedSyncs(),
  });
  const sync = createSyncController({
    adapter,
    reportError: feedback.report,
    locale: options.locale ?? (() => "en"),
    canStartSync: () => watchedFolders.hasFolders,
    onSyncSettled: () => {
      void timeline.refresh();
      void explore.refreshIfActive();
      void tags.refreshIfActive();
      return watchedFolders.refresh();
    },
  });
  const timeline = createTimelineController(adapter);
  const filteredTimelines = createFilteredTimelineControllers(adapter);
  const people = createPeopleController(adapter);
  const places = createPlacesController(adapter);
  const tags = createTagsController(adapter, { isActive: () => options.isTagsActive?.() ?? false });
  const memories = createMemoriesController(adapter);
  const allMemories = createAllMemoriesController(adapter);
  const explore = createExploreController({ people, places, memories, isActive: () => options.isExploreActive?.() ?? false });
  const map = createMapController(adapter);
  const albumCatalogue = createAlbumCatalogueController(adapter);
  const search = createSearchController(adapter);

  const userState: { clear(): void }[] = [
    targetAlbumCatalogue,
    feedback,
    removable,
    sync,
    watchedFolders,
    timeline,
    ...Object.values(filteredTimelines),
    people,
    places,
    tags,
    memories,
    allMemories,
    map,
    albumCatalogue,
    search,
  ];

  function clear() {
    for (const controller of userState) controller.clear();
    options.resetView?.();
  }

  const auth = createAuthController({
    adapter,
    formatError: feedback.formatError,
    reportError: feedback.report,
    t: (key) => t(key),
    virtualLibraryEnabled: () => options.isVirtualLibraryEnabled?.() ?? false,
    onLogin: async () => {
      clear();
      sync.startSubscriptions();
      await Promise.all([
        timeline.refresh(),
        sync.hydrate(),
        sync.refreshFailedSyncs(),
        watchedFolders.refresh(),
      ]);
    },
    onLogout: clear,
  });

  function startLifecycle() {
    sync.startLifecycle();
    watchedFolders.startLifecycle();
    void removable.startSubscriptions();
  }

  function destroy() {
    sync.destroy();
    watchedFolders.destroy();
    removable.destroy();
  }

  return {
    auth,
    feedback,
    targetAlbumCatalogue,
    removable,
    watchedFolders,
    sync,
    timeline,
    filteredTimelines,
    people,
    places,
    tags,
    memories,
    allMemories,
    explore,
    map,
    albumCatalogue,
    search,
    startLifecycle,
    destroy,
  };
}
