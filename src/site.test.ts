import { describe, expect, it } from "vitest";
import { readFileSync, existsSync } from "node:fs";
import { resolve } from "node:path";

describe("landing page", () => {
  const root = resolve(process.cwd());
  const landingPagePath = resolve(root, "site/index.html");
  const content = readFileSync(landingPagePath, "utf-8");
  const document = new DOMParser().parseFromString(content, "text/html");

  it("provides document title and responsive viewport metadata", () => {
    expect(document.title).toBe("Lymic - Open-Source Desktop Client for Immich");
    expect(document.querySelector('meta[name="viewport"]')?.getAttribute("content")).toBe(
      "width=device-width, initial-scale=1",
    );
  });

  it("references existing landing-page assets", () => {
    const localAssets = [
      document.querySelector('link[rel="icon"]')?.getAttribute("href"),
      document.querySelector("main img")?.getAttribute("src"),
    ];

    expect(localAssets).toEqual(["assets/favicon.png", "assets/logo.png"]);
    for (const asset of localAssets) {
      expect(existsSync(resolve(root, "site", asset!))).toBe(true);
    }
  });

  it("presents the product value and primary calls to action", () => {
    expect(document.querySelector("main h1")?.textContent).toMatch(/desktop client for Immich/i);
    expect(document.querySelector("main")?.textContent).toMatch(/open.source/i);
    expect(document.querySelector("main")?.textContent).toMatch(/synchroniz/i);

    const links = Array.from(document.querySelectorAll("main a")).map((link) =>
      link.getAttribute("href"),
    );
    expect(links).toContain("https://github.com/xXRoxXeRXx/lymic/releases");
    expect(links).toContain("https://github.com/xXRoxXeRXx/lymic");
  });

  it("adapts its responsive layout and colors to the viewport and system theme", () => {
    expect(content).toMatch(/@media\s*\([^)]*prefers-color-scheme:\s*dark[^)]*\)/i);
    expect(content).toMatch(/@media\s*\([^)]*max-width:/i);
  });
});

describe("public privacy and support pages", () => {
  const root = resolve(process.cwd());
  const privacyPath = resolve(root, "site/privacy/index.html");
  const supportPath = resolve(root, "site/support/index.html");

  it("provides a privacy page meeting Microsoft Store requirements", () => {
    expect(existsSync(privacyPath)).toBe(true);
    const content = readFileSync(privacyPath, "utf-8");

    // Must declare no advertising, tracking, or analytics telemetry
    expect(content).toMatch(/no advertising/i);
    expect(content).toMatch(/tracking/i);
    expect(content).toMatch(/telemetry/i);

    // Must describe local settings, sync progress, audit data
    expect(content).toMatch(/local/i);
    expect(content).toMatch(/synchronization/i);
    expect(content).toMatch(/audit/i);

    // Must describe native credential storage (keyring)
    expect(content).toMatch(/keyring|credential manager/i);

    // Must describe transfer only to configured Immich server
    expect(content).toMatch(/Immich/i);
  });

  it("provides a support page with durable help channels", () => {
    expect(existsSync(supportPath)).toBe(true);
    const content = readFileSync(supportPath, "utf-8");

    // Must provide durable contact and support mechanisms
    expect(content).toMatch(/github\.com\/xXRoxXeRXx\/lymic/i);
    expect(content).toMatch(/issues|support/i);
  });

  it("provides a complete Microsoft Store listing with localized descriptions, URLs, and screenshot assets", () => {
    const listingPath = resolve(root, "docs/store/listing.md");
    expect(existsSync(listingPath)).toBe(true);
    const listing = readFileSync(listingPath, "utf-8");

    // Check localized descriptions
    expect(listing).toContain("English (en-US)");
    expect(listing).toContain("Deutsch (de-DE)");

    // Check independent disclaimer in both languages
    expect(listing).toMatch(/independent.*Immich/i);
    expect(listing).toMatch(/unabhängiges.*Immich/i);

    // Check required public URLs
    expect(listing).toContain("https://xxroxxerxx.github.io/lymic/privacy/");
    expect(listing).toContain("https://xxroxxerxx.github.io/lymic/support/");

    // Check category & all-ages content
    expect(listing).toContain("Photo & video");
    expect(listing).toContain("All ages");

    // Check screenshots exist
    const screenshots = [
      "docs/store/assets/01_signin.png",
      "docs/store/assets/02_watched_folders.png",
      "docs/store/assets/03_synchronization.png",
      "docs/store/assets/04_settings.png",
    ];
    for (const screenshot of screenshots) {
      expect(existsSync(resolve(root, screenshot))).toBe(true);
      expect(listing).toContain(screenshot);
    }
  });

  it("records private Flight validation covering all acceptance criteria", () => {
    const flightDocPath = resolve(root, "docs/store/flight-validation.md");
    expect(existsSync(flightDocPath)).toBe(true);
    const flightDoc = readFileSync(flightDocPath, "utf-8");

    // Flight criteria verification
    expect(flightDoc).toMatch(/xXRoxXeRXx\.Lymic/);
    expect(flightDoc).toMatch(/broadFileSystemAccess/);
    expect(flightDoc).toMatch(/runFullTrust/);
    expect(flightDoc).toMatch(/Watched Folders/i);
    expect(flightDoc).toMatch(/Synchronization/i);
    expect(flightDoc).toMatch(/PENDING/i);
    expect(flightDoc).toMatch(/Issue #77/i);
  });

  it("records public Microsoft Store release linking Flight evidence and ADR", () => {
    const releaseDocPath = resolve(root, "docs/store/release-record.md");
    expect(existsSync(releaseDocPath)).toBe(true);
    const releaseDoc = readFileSync(releaseDocPath, "utf-8");

    // Public release record verification
    expect(releaseDoc).toMatch(/xXRoxXeRXx\.Lymic/);
    expect(releaseDoc).toMatch(/Photo & video/i);
    expect(releaseDoc).toMatch(/Worldwide/i);
    expect(releaseDoc).toMatch(/ADR-0004/i);
    expect(releaseDoc).toMatch(/flight-validation\.md/);
    expect(releaseDoc).toMatch(/justification\.md/);
  });
});
