import { describe, expect, it } from "vitest";
import { readFileSync, existsSync } from "node:fs";
import { resolve } from "node:path";
import {
  validateSnapcraftConfig,
  type ExpectedSnapcraftProperties,
} from "./snapcraft-validator";

const SAMPLE_VALID_SNAPCRAFT = `name: lymic
title: Lymic
base: core22
version: '0.22.0'
summary: High-performance desktop sync client for Immich
description: |
  Lymic is an unofficial desktop client for uploading and synchronizing local
  media with an Immich server.
grade: stable
confinement: strict
license: MIT
icon: snap/gui/icon.png
website: https://xxroxxerxx.github.io/lymic/
source-code: https://github.com/xXRoxXeRXx/lymic
issues: https://github.com/xXRoxXeRXx/lymic/issues

layout:
  /usr/lib/$SNAPCRAFT_ARCH_TRIPLET/webkit2gtk-4.1:
    bind: $SNAP/usr/lib/$SNAPCRAFT_ARCH_TRIPLET/webkit2gtk-4.1
  /usr/libexec/webkit2gtk-4.1:
    bind: $SNAP/usr/libexec/webkit2gtk-4.1

apps:
  lymic:
    command: bin/lymic
    desktop: snap/gui/lymic.desktop
    extensions: [gnome]
    plugs:
      - network
      - home
      - removable-media
      - password-manager-service
`;

describe("snapcraft validator", () => {
  const defaultExpected: ExpectedSnapcraftProperties = {
    name: "lymic",
    version: "0.22.0",
    confinement: "strict",
    base: "core22",
  };

  it("validates a compliant Snapcraft configuration", () => {
    const result = validateSnapcraftConfig(SAMPLE_VALID_SNAPCRAFT, defaultExpected);

    expect(result.valid).toBe(true);
    expect(result.errors).toHaveLength(0);
  });

  it("detects wrong snap name", () => {
    const result = validateSnapcraftConfig(SAMPLE_VALID_SNAPCRAFT, {
      ...defaultExpected,
      name: "wrong-name",
    });

    expect(result.valid).toBe(false);
    expect(result.errors).toContain("Snap name mismatch: expected 'wrong-name', got 'lymic'");
  });

  it("detects missing GNOME extension", () => {
    const withoutGnome = SAMPLE_VALID_SNAPCRAFT.replace("extensions: [gnome]", "");
    const result = validateSnapcraftConfig(withoutGnome, defaultExpected);

    expect(result.valid).toBe(false);
    expect(result.errors).toContain("Missing required extension: 'gnome' under apps.lymic");
  });

  it("detects missing required plugs", () => {
    const withoutHome = SAMPLE_VALID_SNAPCRAFT.replace("- home", "");
    const result = validateSnapcraftConfig(withoutHome, defaultExpected);

    expect(result.valid).toBe(false);
    expect(result.errors).toContain("Missing required plug: 'home'");
  });

  it("detects missing removable-media plug", () => {
    const withoutRemovable = SAMPLE_VALID_SNAPCRAFT.replace("- removable-media", "");
    const result = validateSnapcraftConfig(withoutRemovable, defaultExpected);

    expect(result.valid).toBe(false);
    expect(result.errors).toContain("Missing required plug: 'removable-media'");
  });

  it("detects missing network plug", () => {
    const withoutNetwork = SAMPLE_VALID_SNAPCRAFT.replace("- network", "");
    const result = validateSnapcraftConfig(withoutNetwork, defaultExpected);

    expect(result.valid).toBe(false);
    expect(result.errors).toContain("Missing required plug: 'network'");
  });

  it("detects missing WebKitGTK layout binding", () => {
    const withoutLayout = SAMPLE_VALID_SNAPCRAFT.replace(/layout:[\s\S]*?(?=apps:)/, "");
    const result = validateSnapcraftConfig(withoutLayout, defaultExpected);

    expect(result.valid).toBe(false);
    expect(
      result.errors.some((e) => e.includes("webkit2gtk-4.1") && e.includes("layout")),
    ).toBe(true);
  });

  it("validates the repository's snap/snapcraft.yaml file", () => {
    const snapcraftPath = resolve(process.cwd(), "snap/snapcraft.yaml");
    expect(existsSync(snapcraftPath), "snap/snapcraft.yaml should exist").toBe(true);

    const packageJson = JSON.parse(
      readFileSync(resolve(process.cwd(), "package.json"), "utf-8"),
    );
    const yamlContent = readFileSync(snapcraftPath, "utf-8");

    const result = validateSnapcraftConfig(yamlContent, {
      name: "lymic",
      version: packageJson.version,
      confinement: "strict",
      base: "core22",
    });

    expect(result.valid).toBe(true);
    expect(result.errors).toHaveLength(0);
  });
});

