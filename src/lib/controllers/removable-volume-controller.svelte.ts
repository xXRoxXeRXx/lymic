import { listen } from "@tauri-apps/api/event";
import { isPermissionGranted, sendNotification } from "@tauri-apps/plugin-notification";
import type { DetectedVolume } from "../contracts";

export interface RemovableVolumeControllerOptions {
  isAppInBackground?: () => boolean;
  t: (key: string, params?: Record<string, string>) => string;
}

export function createRemovableVolumeController(options: RemovableVolumeControllerOptions) {
  let detectedVolumes = $state<DetectedVolume[]>([]);
  const dismissedVolumeIds = new Set<string>();
  let unlisten: (() => void) | null = null;
  let startingPromise: Promise<void> | null = null;
  let disposed = false;
  const resolvedUnlisteners: (() => void)[] = [];

  const currentPrompt = $derived(detectedVolumes.length > 0 ? detectedVolumes[0] : null);

  const isAppInBackground = options.isAppInBackground ?? (() => {
    if (typeof document !== "undefined") {
      return document.hidden || !document.hasFocus();
    }
    return false;
  });

  async function notifyArrival(volume: DetectedVolume) {
    if (!isAppInBackground()) return;

    try {
      const hasPermission = await isPermissionGranted();
      if (hasPermission) {
        sendNotification({
          title: options.t("removable_card_detected_title", { name: volume.displayName }),
          body: options.t("removable_card_detected_body", {
            name: volume.displayName,
            path: volume.mountPath,
          }),
        });
      }
    } catch (error) {
      console.error("Failed to show desktop notification for removable volume", error);
    }
  }

  function handleVolumeDetected(volume: DetectedVolume) {
    if (dismissedVolumeIds.has(volume.volumeId)) {
      return;
    }
    if (detectedVolumes.some((v) => v.volumeId === volume.volumeId)) {
      return;
    }
    detectedVolumes.push(volume);
    void notifyArrival(volume);
  }

  function handleVolumeRemoved(idOrPath: string) {
    detectedVolumes = detectedVolumes.filter(
      (v) => v.volumeId !== idOrPath && v.mountPath !== idOrPath
    );
  }

  function stopResolvedListeners() {
    resolvedUnlisteners.splice(0).forEach((stopListening) => stopListening());
  }

  function retainOrStopListener(stopListening: () => void) {
    if (disposed) stopListening();
    else resolvedUnlisteners.push(stopListening);
  }

  function startSubscriptions(): Promise<void> {
    if (disposed || unlisten) return Promise.resolve();
    if (startingPromise) return startingPromise;

    startingPromise = Promise.all([
      listen<DetectedVolume>("removable-volume-detected", (event) => {
        handleVolumeDetected(event.payload);
      }).then(retainOrStopListener),
      listen<string>("removable-volume-removed", (event) => {
        handleVolumeRemoved(event.payload);
      }).then(retainOrStopListener),
    ])
      .then(() => {
        if (!disposed) {
          unlisten = stopResolvedListeners;
        }
      })
      .finally(() => {
        startingPromise = null;
      });

    return startingPromise;
  }

  function dismiss(volumeId: string) {
    dismissedVolumeIds.add(volumeId);
    detectedVolumes = detectedVolumes.filter((v) => v.volumeId !== volumeId);
  }

  function clear() {
    detectedVolumes = [];
    dismissedVolumeIds.clear();
  }

  function destroy() {
    disposed = true;
    const stopListening = unlisten ?? stopResolvedListeners;
    unlisten = null;
    stopListening();
  }

  return {
    get detectedVolumes() {
      return detectedVolumes;
    },
    get currentPrompt() {
      return currentPrompt;
    },
    startSubscriptions,
    dismiss,
    clear,
    destroy,
  };
}
