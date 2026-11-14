import { createAlbumCatalogueController } from "./album-catalogue-controller.svelte";
import { createAuthController } from "./auth-controller.svelte";
import { createFavoritesTimelineController } from "./favorites-timeline-controller.svelte";
import { createFeedbackController } from "./feedback-controller.svelte";
import { createMapController } from "./map-controller.svelte";
import { createPeopleController } from "./people-controller.svelte";
import { createRemovableVolumeController } from "./removable-volume-controller.svelte";
import { createSyncController } from "./sync-controller.svelte";
import { createTargetAlbumCatalogue } from "./target-album-catalogue.svelte";
import { createTimelineController } from "./timeline-controller.svelte";
import { createWatchedFoldersController } from "./watched-folders-controller.svelte";
import type {
  AlbumCatalogueDesktopAdapter,
  AlbumsDesktopAdapter,
  AuthDesktopAdapter,
  FavoritesTimelineDesktopAdapter,
  MapDesktopAdapter,
  PeopleDesktopAdapter,
  RemovableVolumeDesktopAdapter,
  SyncDesktopAdapter,
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
  FavoritesTimelineDesktopAdapter &
  MapDesktopAdapter &
  PeopleDesktopAdapter &
  RemovableVolumeDesktopAdapter;

export function createSessionController(options: {
  adapter: SessionDesktopAdapter;
  t(key: string, params?: Record<string, string>): string;
  locale?(): string;
  resetView?(): void;
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
      return watchedFolders.refresh();
    },
  });
  const timeline = createTimelineController(adapter);
  const favorites = createFavoritesTimelineController(adapter);
  const people = createPeopleController(adapter);
  const map = createMapController(adapter);
  const albumCatalogue = createAlbumCatalogueController(adapter);

  const userState: { clear(): void }[] = [
    targetAlbumCatalogue,
    feedback,
    removable,
    sync,
    watchedFolders,
    timeline,
    favorites,
    people,
    map,
    albumCatalogue,
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
    get isAuthenticated() { return auth.isAuthenticated; },
    get serverUrl() { return auth.serverUrl; },
    set serverUrl(value: string) { auth.serverUrl = value; },
    get currentUserName() { return auth.currentUserName; },
    get profileImage() { return auth.profileImage; },
    get apiKey() { return auth.apiKey; },
    set apiKey(value: string) { auth.apiKey = value; },
    get allowLocalHttp() { return auth.allowLocalHttp; },
    set allowLocalHttp(value: boolean) { auth.allowLocalHttp = value; },
    get cloudflareClientId() { return auth.cloudflareClientId; },
    set cloudflareClientId(value: string) { auth.cloudflareClientId = value; },
    get cloudflareClientSecret() { return auth.cloudflareClientSecret; },
    set cloudflareClientSecret(value: string) { auth.cloudflareClientSecret = value; },
    get isLoggingIn() { return auth.isLoggingIn; },
    get loginError() { return auth.loginError; },
    get loginErrorCode() { return auth.loginErrorCode; },
    hydrate: auth.hydrate,
    login: auth.login,
    logout: auth.logout,
    feedback,
    targetAlbumCatalogue,
    removable,
    watchedFolders,
    sync,
    timeline,
    favorites,
    people,
    map,
    albumCatalogue,
    startLifecycle,
    destroy,
  };
}
