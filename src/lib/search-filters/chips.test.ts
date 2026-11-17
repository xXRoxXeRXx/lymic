import { describe, expect, it } from "vitest";
import type { SearchFilters } from "../contracts";
import { activeChips, chipSummary, clearChip, editorOfFilter, searchChips } from "./chips";
import type { SearchChipContext } from "./types";

const context: SearchChipContext = {
  t: (key, values) => (values ? `${key}(${Object.values(values).join(",")})` : key),
  personName: (id) => ({ anna: "Anna", ben: "Ben" })[id] ?? null,
  formatDay: (day) => `<${day}>`,
};

function summaryOf(chipId: string, filters: SearchFilters) {
  const chip = searchChips.find((candidate) => candidate.id === chipId)!;
  return chipSummary(chip, filters, context);
}

describe("Filter Chip summaries", () => {
  it("is null for every chip while no Search Filter is set", () => {
    for (const chip of searchChips) expect(chipSummary(chip, {}, context)).toBeNull();
    expect(activeChips({}, context)).toEqual([]);
  });

  it("names the first known person and counts the others", () => {
    expect(summaryOf("person", { personIds: ["anna"] })).toBe("Anna");
    expect(summaryOf("person", { personIds: ["anna", "ben", "x"] })).toBe("Anna +2");
    expect(summaryOf("person", { personIds: ["x", "ben"] })).toBe("Ben +1");
  });

  it("falls back to the count when no chosen person is known", () => {
    expect(summaryOf("person", { personIds: ["x", "y"] })).toBe("search_chip_chosen(2)");
  });

  it("shows how many Tags are chosen", () => {
    expect(summaryOf("tag", { tagIds: ["a", "b", "c"] })).toBe("search_chip_chosen(3)");
  });

  it("shows a date range, or the one bound that is set", () => {
    expect(summaryOf("date", { date: { from: "2024-03-01", to: "2024-04-15" } })).toBe("<2024-03-01> – <2024-04-15>");
    expect(summaryOf("date", { date: { from: "2024-03-01" } })).toBe("search_chip_date_from(<2024-03-01>)");
    expect(summaryOf("date", { date: { to: "2024-04-15" } })).toBe("search_chip_date_to(<2024-04-15>)");
  });

  it("ignores a malformed date bound", () => {
    expect(summaryOf("date", { date: { from: "garbage" } })).toBeNull();
  });

  it("shows the media type, the Rating and the Location from the finest part to the coarsest", () => {
    expect(summaryOf("mediaType", { mediaType: "video" })).toBe("search_filter_media_video");
    expect(summaryOf("rating", { rating: 4 })).toBe("4 ★");
    expect(summaryOf("location", { location: { country: "Germany", city: "Berlin" } })).toBe("Berlin, Germany");
  });

  it("shows the camera as make and model, and counts the lens as one more", () => {
    expect(summaryOf("camera", { camera: { make: "Canon", model: "EOS R5" } })).toBe("Canon EOS R5");
    expect(summaryOf("camera", { camera: { make: "Canon", lensModel: "RF 50mm" } })).toBe("Canon +1");
    expect(summaryOf("camera", { camera: { lensModel: "RF 50mm" } })).toBe("RF 50mm");
  });

  it("combines the three Display options into one value", () => {
    expect(summaryOf("display", { favorite: true, unfiled: true })).toBe("search_filter_favorite, search_filter_unfiled");
    expect(summaryOf("display", { archived: true })).toBe("search_filter_archived");
  });

  it("lists only the active chips, in panel order", () => {
    const active = activeChips({ rating: 3, personIds: ["anna"], favorite: true }, context);

    expect(active.map(({ chip }) => chip.id)).toEqual(["person", "rating", "display"]);
  });
});

describe("clearChip", () => {
  it("unsets every Search Filter of the chip and leaves the others alone", () => {
    const filters: SearchFilters = { favorite: true, archived: true, rating: 2 };

    clearChip(searchChips.find((chip) => chip.id === "display")!, filters);

    expect(filters.favorite).toBeUndefined();
    expect(filters.archived).toBeUndefined();
    expect(filters.rating).toBe(2);
  });
});

describe("editorOfFilter", () => {
  it("names the editor that holds a Search Filter", () => {
    expect(editorOfFilter("date")).toBe("date");
    expect(editorOfFilter("archived")).toBe("display");
    expect(editorOfFilter("personIds")).toBe("person");
  });
});
