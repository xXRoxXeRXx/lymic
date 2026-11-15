import { describe, expect, it } from "vitest";
import type { Memory } from "./contracts";
import { memoryTitle } from "./memory-title";

const translate = (key: string, values?: Record<string, string | number>) =>
  `${key}${values ? `(${Object.entries(values).map(([name, value]) => `${name}=${value}`).join(",")})` : ""}`;

function memory(overrides: Partial<Memory>): Memory {
  return {
    id: "m",
    kind: "onThisDay",
    memoryAt: "2020-11-15T00:00:00.000Z",
    year: 2020,
    personName: null,
    coverAssetId: "a",
    timeline: { buckets: [], assetsByBucket: {} },
    ...overrides,
  };
}

const title = (value: Memory, now: Date) => memoryTitle(value, translate, { locale: "en", now });

describe("memoryTitle", () => {
  it("counts years for an on-this-day Memory on its anniversary", () => {
    expect(title(memory({}), new Date(2026, 10, 15, 8))).toBe("memory_years_ago(count=6)");
  });

  it("uses the singular for one year ago", () => {
    expect(title(memory({ year: 2025, memoryAt: "2025-11-15T00:00:00.000Z" }), new Date(2026, 10, 15, 8))).toBe("memory_year_ago");
  });

  it("uses a medium date for an on-this-day Memory on another day", () => {
    expect(title(memory({}), new Date(2026, 10, 16, 8))).toBe("Nov 15, 2020");
  });

  it("names the person for a birthday Memory", () => {
    expect(title(memory({ kind: "birthday", personName: "Ada" }), new Date(2026, 10, 15))).toBe("memory_birthday(name=Ada)");
  });

  it("falls back to the date for a nameless birthday and for unknown types", () => {
    expect(title(memory({ kind: "birthday" }), new Date(2026, 10, 15))).toBe("Nov 15, 2020");
    expect(title(memory({ kind: "unknown", year: null }), new Date(2026, 10, 15))).toBe("Nov 15, 2020");
  });

  it("uses a generic title when the Memory has no date at all", () => {
    expect(title(memory({ kind: "unknown", memoryAt: null, year: null }), new Date(2026, 10, 15))).toBe("memory_untitled");
  });

  it("does not claim a past anniversary when the year is unknown or not in the past", () => {
    expect(title(memory({ year: null }), new Date(2026, 10, 15))).toBe("Nov 15, 2020");
    expect(title(memory({ year: 2026, memoryAt: "2026-11-15T00:00:00.000Z" }), new Date(2026, 10, 15))).toBe("Nov 15, 2026");
  });
});
