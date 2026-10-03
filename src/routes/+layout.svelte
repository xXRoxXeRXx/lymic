<script>
  import "../app.css";
  import "#lib/i18n/index.js";
  import { waitLocale } from "svelte-i18n";
</script>

{#await waitLocale()}
  <div class="h-screen w-screen flex items-center justify-center bg-[#F1F5F9]">
    <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-blue-600"></div>
  </div>
{:then}
  <slot />
{/await}
