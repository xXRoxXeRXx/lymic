import type { SearchFilterDefinition } from "./types";

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

/** A Search Filter that is a list of ids which must all apply (people, Tags); no id adds no condition. */
export function allIdsFilter(key: "personIds" | "tagIds"): SearchFilterDefinition {
  return {
    key,
    apply(filters, request) {
      const ids = filters[key];
      if (ids?.length) request[key] = [...ids];
    },
  };
}
