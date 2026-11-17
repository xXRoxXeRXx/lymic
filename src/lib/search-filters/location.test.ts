import { describe, expect, it } from "vitest";
import { buildRequestFilters } from "./index";
import { setLocationPart } from "./location";

describe("Location Search Filter", () => {
  it("adds the chosen country and state to the request", () => {
    expect(buildRequestFilters({ location: { country: "Italy", state: "Tuscany" } })).toEqual({
      location: { country: "Italy", state: "Tuscany" },
    });
  });

  it("adds either part alone and nothing when none is chosen", () => {
    expect(buildRequestFilters({ location: { country: "Italy" } })).toEqual({ location: { country: "Italy" } });
    expect(buildRequestFilters({ location: { state: "Bavaria" } })).toEqual({ location: { state: "Bavaria" } });
    expect(buildRequestFilters({ location: { country: "" } })).toEqual({});
    expect(buildRequestFilters({ location: {} })).toEqual({});
    expect(buildRequestFilters({})).toEqual({});
  });

  it("sets and clears one part, leaving the filter unset when no part is left", () => {
    const withCountry = setLocationPart(undefined, "country", "Italy");
    expect(withCountry).toEqual({ country: "Italy" });
    expect(setLocationPart(withCountry, "state", "Tuscany")).toEqual({ country: "Italy", state: "Tuscany" });
    expect(setLocationPart(withCountry, "country", undefined)).toBeUndefined();
  });

  it("drops the chosen state when a different country is chosen, because it may not exist in that country", () => {
    const location = { country: "Italy", state: "Tuscany" };

    expect(setLocationPart(location, "country", "Germany")).toEqual({ country: "Germany" });
    expect(setLocationPart(location, "country", "Italy")).toEqual(location);
  });

  it("keeps the chosen state when the country is cleared", () => {
    expect(setLocationPart({ country: "Italy", state: "Tuscany" }, "country", undefined)).toEqual({ state: "Tuscany" });
  });
});
