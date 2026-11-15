import type { Memory } from "./contracts";

type Translate = (key: string, values?: Record<string, string | number>) => string;

/**
 * The card and page title of a Memory: "N years ago" for an on-this-day Memory
 * on its anniversary, the person's name for a birthday Memory, and the Memory's
 * medium date otherwise (also the fallback for unknown types).
 */
export function memoryTitle(memory: Memory, translate: Translate, options: { locale: string; now?: Date }): string {
  const at = memory.memoryAt ? new Date(memory.memoryAt) : null;
  const validAt = at && !Number.isNaN(at.getTime()) ? at : null;
  const now = options.now ?? new Date();

  if (memory.kind === "birthday" && memory.personName) {
    return translate("memory_birthday", { name: memory.personName });
  }
  if (memory.kind === "onThisDay" && validAt && memory.year !== null) {
    // Immich stamps the day a Memory looks back on in UTC; compare it with the user's local today.
    const onAnniversary = validAt.getUTCMonth() === now.getMonth() && validAt.getUTCDate() === now.getDate();
    const yearsAgo = now.getFullYear() - memory.year;
    if (onAnniversary && yearsAgo === 1) return translate("memory_year_ago");
    if (onAnniversary && yearsAgo > 1) return translate("memory_years_ago", { count: yearsAgo });
  }
  if (validAt) {
    return new Intl.DateTimeFormat(options.locale, { dateStyle: "medium", timeZone: "UTC" }).format(validAt);
  }
  return translate("memory_untitled");
}

/**
 * `memoryTitle` bound to svelte-i18n: pass the `$t` formatter and the `$locale`
 * value so the title follows the active locale (an unset locale means English).
 */
export function localizedMemoryTitle(
  memory: Memory,
  t: (key: string, options?: { values?: Record<string, string | number> }) => string,
  locale: string | null | undefined,
): string {
  return memoryTitle(memory, (key, values) => t(key, { values }), { locale: locale ?? "en" });
}
