<script lang="ts">
  import { locale, t } from "svelte-i18n";
  import { toBcp47Locale } from "../i18n";
  import { trashFeedbackMessage, type TrashFeedback } from "./remote-asset-trash";

  interface Props {
    feedback: TrashFeedback;
    /** Layout of the host view (margins); the colours belong to the notice. */
    class?: string;
  }

  let { feedback, class: className = "" }: Props = $props();
  const numberLocale = $derived(toBcp47Locale($locale));
  const isSuccess = $derived(feedback.kind === "success");
</script>

<p class="{className} break-words rounded-lg px-3 py-2 text-sm leading-5 {isSuccess ? 'bg-emerald-50 text-emerald-800 dark:bg-emerald-950/30 dark:text-emerald-200' : 'bg-red-50 text-red-800 dark:bg-red-950/30 dark:text-red-200'}" role={isSuccess ? "status" : "alert"}>
  {trashFeedbackMessage(feedback, $t, numberLocale)}
</p>
