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
});
