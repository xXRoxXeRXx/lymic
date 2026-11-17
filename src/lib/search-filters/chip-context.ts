import type { SearchChipContext } from "./types";

type Translate = (key: string, options?: { values?: Record<string, string | number> }) => string;

/** The surroundings a Filter Chip summary needs: the active translation and locale, and the Recognized People loaded so far. */
export function chipContext(translate: Translate, locale: string | null | undefined, people: readonly { id: string; name: string | null }[]): SearchChipContext {
  const names = new Map(people.map((person) => [person.id, person.name] as const));
  const format = new Intl.DateTimeFormat(locale ?? undefined, { dateStyle: "medium" });
  return {
    t: (key, values) => translate(key, values ? { values } : undefined),
    personName: (id) => names.get(id) || null,
    formatDay(day) {
      const [year, month, date] = day.split("-").map(Number);
      return format.format(new Date(year, month - 1, date));
    },
  };
}
