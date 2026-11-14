import { createAuthController } from "./auth-controller.svelte";
import type { AuthDesktopAdapter } from "../tauri/desktop-adapter";

export function createSessionController(options: {
  adapter: AuthDesktopAdapter;
  formatError(error: unknown): string;
  reportError(error: unknown): void;
  t(key: string): string;
  state: { clear(): void }[];
  refresh: (() => Promise<void>)[];
  resetView(): void;
}) {
  function clear() {
    for (const state of options.state) state.clear();
    options.resetView();
  }

  return createAuthController({
    ...options,
    onLogin: async () => {
      clear();
      await Promise.all(options.refresh.map((refresh) => refresh()));
    },
    onLogout: clear,
  });
}
