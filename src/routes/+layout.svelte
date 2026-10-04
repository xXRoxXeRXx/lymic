<script>
  import "../app.css";
  import "#lib/i18n/index.js";
  import { waitLocale } from "svelte-i18n";
  import { onMount } from "svelte";
  import { setupBrowserInteractions } from "#lib/prevent-browser-defaults.js";

  if (typeof window !== "undefined") {
    const savedTheme = localStorage.getItem("lymic-theme");
    const themePreference = savedTheme === "light" || savedTheme === "dark" || savedTheme === "system"
      ? savedTheme
      : "system";
    const effectiveTheme = themePreference === "system"
      ? (window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light")
      : themePreference;
    document.documentElement.dataset.theme = effectiveTheme;

    if (savedTheme && savedTheme !== themePreference) {
      localStorage.removeItem("lymic-theme");
    }
  }

  onMount(() => {
    return setupBrowserInteractions();
  });
</script>

{#await waitLocale()}
  <div class="h-screen w-screen flex items-center justify-center bg-[#F1F5F9] dark:bg-slate-950">
    <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-blue-600"></div>
  </div>
{:then}
  <slot />
{/await}
