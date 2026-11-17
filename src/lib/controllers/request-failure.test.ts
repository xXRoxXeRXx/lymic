import { describe, expect, it } from "vitest";
import { classifyRequestFailure, messageForFailure } from "./request-failure";

describe("classifyRequestFailure for the Tags section", () => {
  it("maps a permission denial naming tag.read to permission_denied", () => {
    expect(classifyRequestFailure("tags_permission_denied:tag.read", "tags")).toBe("permission_denied");
  });

  it("maps authentication-required and unavailable to unavailable", () => {
    expect(classifyRequestFailure("tags_authentication_required", "tags")).toBe("unavailable");
    expect(classifyRequestFailure("tags_unavailable", "tags")).toBe("unavailable");
  });

  it("maps anything else to a generic error", () => {
    expect(classifyRequestFailure("tags_invalid_response", "tags")).toBe("error");
    expect(classifyRequestFailure("tags_permission_denied", "tags")).toBe("error");
    expect(classifyRequestFailure("places_permission_denied:place.read", "tags")).toBe("error");
    expect(classifyRequestFailure(new Error("boom"), "tags")).toBe("error");
    expect(classifyRequestFailure(undefined, "tags")).toBe("error");
  });

  it("reads the code from an Error message as well as from a string", () => {
    expect(classifyRequestFailure(new Error("tags_permission_denied:tag.read"), "tags")).toBe("permission_denied");
  });
});

describe("messageForFailure", () => {
  const messages = { permission_denied: "denied", unavailable: "offline", error: "broken" };

  it("picks the message of the failure kind", () => {
    expect(messageForFailure("permission_denied", messages)).toBe("denied");
    expect(messageForFailure("unavailable", messages)).toBe("offline");
    expect(messageForFailure("error", messages)).toBe("broken");
  });
});
