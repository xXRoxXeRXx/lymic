import { describe, expect, it } from "vitest";
import { buildRequestFilters } from "./index";
import { setCameraPart } from "./camera";

describe("camera Search Filter", () => {
  it("adds the chosen make, model and lens model to the request", () => {
    expect(buildRequestFilters({ camera: { make: "FUJIFILM", model: "X100V", lensModel: "23mm F2" } })).toEqual({
      camera: { make: "FUJIFILM", model: "X100V", lensModel: "23mm F2" },
    });
  });

  it("adds only the parts that are chosen and nothing when none is", () => {
    expect(buildRequestFilters({ camera: { lensModel: "RF 50mm" } })).toEqual({ camera: { lensModel: "RF 50mm" } });
    expect(buildRequestFilters({ camera: { make: "" } })).toEqual({});
    expect(buildRequestFilters({ camera: {} })).toEqual({});
    expect(buildRequestFilters({})).toEqual({});
  });

  it("sets and clears one part, leaving the filter unset when no part is left", () => {
    const withMake = setCameraPart(undefined, "make", "Canon");
    expect(withMake).toEqual({ make: "Canon" });
    expect(setCameraPart(withMake, "lensModel", "RF 50mm")).toEqual({ make: "Canon", lensModel: "RF 50mm" });
    expect(setCameraPart(withMake, "make", undefined)).toBeUndefined();
  });

  it("drops the chosen model when a different make is chosen, because it may not exist for that make", () => {
    const camera = { make: "Canon", model: "EOS R5", lensModel: "RF 50mm" };

    expect(setCameraPart(camera, "make", "FUJIFILM")).toEqual({ make: "FUJIFILM", lensModel: "RF 50mm" });
    expect(setCameraPart(camera, "make", "Canon")).toEqual(camera);
  });

  it("keeps the chosen model when the make is cleared", () => {
    expect(setCameraPart({ make: "Canon", model: "EOS R5" }, "make", undefined)).toEqual({ model: "EOS R5" });
  });
});
