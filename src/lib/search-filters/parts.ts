import type { SearchChipContext, SearchFilterDefinition } from "./types";

/**
 * A multi-part Search Filter (camera, Location) with one part set (or cleared, with `undefined`); unset when no
 * part is left. Changing the `parent` part to a different value drops the `dependent` part, which may not exist
 * under the new parent (its suggestions are limited by the parent).
 */
export function setPart<T extends object>(
  current: T | undefined,
  part: keyof T,
  value: string | undefined,
  dependency: { parent: keyof T; dependent: keyof T },
): T | undefined {
  const next = { ...current, [part]: value } as Record<keyof T, string | undefined>;
  const previous = current?.[part] as string | undefined;
  if (part === dependency.parent && value !== undefined && value !== previous) next[dependency.dependent] = undefined;
  for (const key of Object.keys(next) as (keyof T)[]) if (next[key] === undefined) delete next[key];
  return Object.keys(next).length > 0 ? (next as unknown as T) : undefined;
}

/** The non-empty parts of a multi-part Search Filter, in the given order; undefined when none is set. */
export function chosenParts<T extends object>(filter: T | undefined, keys: readonly (keyof T)[]): T | undefined {
  const chosen: Partial<T> = {};
  for (const key of keys) if (filter?.[key]) chosen[key] = filter[key];
  return Object.keys(chosen).length > 0 ? (chosen as T) : undefined;
}

/** A list of chosen ids (people, Tags) with one id toggled: added at the end, or removed; undefined when none is left. */
export function toggleChosenId(chosen: readonly string[] | undefined, id: string): string[] | undefined {
  const current = chosen ?? [];
  const next = current.includes(id) ? current.filter((candidate) => candidate !== id) : [...current, id];
  return next.length > 0 ? next : undefined;
}

/** The first value and how many more there are: "Anna +2". */
export function firstAndMore(values: readonly string[]): string {
  return values.length > 1 ? `${values[0]} +${values.length - 1}` : values[0];
}

/** The summary of a list of chosen ids: the first known name and how many more, or just the count when no name is known. */
export function chosenIdsSummary(ids: readonly string[] | undefined, nameOf: (id: string) => string | null, context: SearchChipContext): string | null {
  if (!ids?.length) return null;
  const firstName = ids.map(nameOf).find((name): name is string => name !== null);
  return firstName === undefined ? context.t("search_chip_chosen", { count: ids.length }) : firstAndMore([firstName, ...ids.slice(1)]);
}

/** A Search Filter that is a list of ids which must all apply (people, Tags); no id adds no condition. */
export function allIdsFilter(key: "personIds" | "tagIds", nameOf: (id: string, context: SearchChipContext) => string | null = () => null): SearchFilterDefinition {
  return {
    key,
    apply(filters, request) {
      const ids = filters[key];
      if (ids?.length) request[key] = [...ids];
    },
    summary: (filters, context) => chosenIdsSummary(filters[key], (id) => nameOf(id, context), context),
  };
}
