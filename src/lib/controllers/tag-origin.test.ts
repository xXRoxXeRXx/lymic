import { describe, expect, it } from "vitest";
import { defaultTagOrigin, recordTagOrigin } from "./tag-origin";

describe("Tag origin", () => {
  it("defaults to the Tags view at the top", () => {
    expect(defaultTagOrigin).toEqual({ view: "tags", top: 0 });
  });

  it("records the view a Tag Timeline is opened from, with its scroll position", () => {
    expect(recordTagOrigin(defaultTagOrigin, { view: "favorites", top: 320 })).toEqual({ view: "favorites", top: 320 });
    expect(recordTagOrigin({ view: "photos", top: 10 }, { view: "tags", top: 40 })).toEqual({ view: "tags", top: 40 });
  });

  it("keeps the original origin when hopping from one Tag Timeline to another", () => {
    const origin = { view: "album-timeline", top: 75 };

    expect(recordTagOrigin(origin, { view: "tag-timeline", top: 0 })).toBe(origin);
  });
});
