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
