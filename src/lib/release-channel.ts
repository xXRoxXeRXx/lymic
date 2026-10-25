import type { ReleaseChannel } from "./tauri/desktop-adapter";

export function getEffectiveReleaseChannel(): ReleaseChannel {
  const env =
    typeof import.meta !== "undefined" && import.meta.env
      ? import.meta.env.VITE_LYMIC_RELEASE_CHANNEL || import.meta.env.LYMIC_RELEASE_CHANNEL
      : undefined;
  if (env === "store") return "store";
  return "github";
}
