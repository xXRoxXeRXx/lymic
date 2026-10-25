import { describe, expect, it } from "vitest";
import { readFileSync, existsSync } from "node:fs";
import { resolve } from "node:path";

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
});

