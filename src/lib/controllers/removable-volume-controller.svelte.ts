import { listen } from "@tauri-apps/api/event";
import { isPermissionGranted, sendNotification } from "@tauri-apps/plugin-notification";
import type { DetectedVolume } from "./types";

export interface RemovableVolumeControllerOptions {
  isAppInBackground?: () => boolean;
  t: (key: string, params?: Record<string, string>) => string;
}

export function createRemovableVolumeController(options: RemovableVolumeControllerOptions) {
  let detectedVolumes = $state<DetectedVolume[]>([]);
  const dismissedVolumeIds = new Set<string>();
  let unlisten: (() => void) | null = null;

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

  async function startSubscriptions() {
    if (unlisten) return;
    const unlistenDetected = await listen<DetectedVolume>("removable-volume-detected", (event) => {
      handleVolumeDetected(event.payload);
    });
    const unlistenRemoved = await listen<string>("removable-volume-removed", (event) => {
      handleVolumeRemoved(event.payload);
    });
    unlisten = () => {
      unlistenDetected();
      unlistenRemoved();
    };
  }

  function dismiss(volumeId: string) {
    dismissedVolumeIds.add(volumeId);
    detectedVolumes = detectedVolumes.filter((v) => v.volumeId !== volumeId);
  }

  function destroy() {
    if (unlisten) {
      unlisten();
      unlisten = null;
    }
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
    destroy,
  };
}
