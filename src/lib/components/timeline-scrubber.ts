import type { TimelineBucket } from "../contracts";

export interface TimelineScrubberYear {
  year: number;
  months: string[];
}

export function buildTimelineScrubberIndex(buckets: TimelineBucket[]): TimelineScrubberYear[] {
  const years = new Map<number, string[]>();

  for (const bucket of buckets) {
    const year = new Date(bucket.timeBucket).getUTCFullYear();
    const months = years.get(year) ?? [];
    months.push(bucket.timeBucket);
    years.set(year, months);
  }

  return [...years.entries()]
    .sort(([left], [right]) => right - left)
    .map(([year, months]) => ({ year, months: months.sort((left, right) => right.localeCompare(left)) }));
}
