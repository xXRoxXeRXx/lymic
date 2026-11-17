import { describe, expect, it } from "vitest";
import { validTagColor } from "./tag-color";

describe("validTagColor", () => {
  it("accepts exactly #RRGGBB", () => {
    expect(validTagColor("#336699")).toBe("#336699");
    expect(validTagColor("#AbCdEf")).toBe("#AbCdEf");
  });

  it("rejects anything else so it never reaches a style attribute", () => {
    for (const color of ["336699", "#369", "#33669900", "#33669g", "red", "#336699; x: y", "", null, undefined]) {
      expect(validTagColor(color)).toBeNull();
    }
  });
});
