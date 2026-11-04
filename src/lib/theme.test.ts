import { afterEach, describe, expect, it, vi } from "vitest";
import { initializeThemeFromStorage } from "./theme";

afterEach(() => {
  localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
  vi.unstubAllGlobals();
});

describe("initializeThemeFromStorage", () => {
  it("applies a valid stored theme", () => {
    localStorage.setItem("lymic-theme", "dark");

    expect(initializeThemeFromStorage()).toBe("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  });

  it("clears an invalid stored theme and applies the system theme", () => {
    localStorage.setItem("lymic-theme", "sepia");
    vi.stubGlobal("matchMedia", vi.fn().mockReturnValue({ matches: false }));

    expect(initializeThemeFromStorage()).toBe("system");
    expect(localStorage.getItem("lymic-theme")).toBeNull();
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  });

  it("applies dark when the system color scheme is dark", () => {
    vi.stubGlobal("matchMedia", vi.fn().mockReturnValue({ matches: true }));

    expect(initializeThemeFromStorage()).toBe("system");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  });
});
