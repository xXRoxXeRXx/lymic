import { describe, expect, it } from "vitest";
import { toggleChosenId } from "./parts";

describe("toggleChosenId", () => {
  it("adds an id that is not chosen yet, after the ones already chosen", () => {
    expect(toggleChosenId(undefined, "a")).toEqual(["a"]);
    expect(toggleChosenId(["a"], "b")).toEqual(["a", "b"]);
  });

  it("removes an id that is chosen, and leaves the filter unset when none is left", () => {
    expect(toggleChosenId(["a", "b"], "a")).toEqual(["b"]);
    expect(toggleChosenId(["a"], "a")).toBeUndefined();
  });
});
