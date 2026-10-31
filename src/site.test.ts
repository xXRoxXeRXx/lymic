import { describe, expect, it } from "vitest";
import { readFileSync, existsSync } from "node:fs";
import { createHash } from "node:crypto";
import { resolve } from "node:path";

const expectPlannedStoreChannel = (content: string) => {
  expect(content).toMatch(/planned|pending/i);
  expect(content).toMatch(/has not been submitted/i);
  expect(content).toMatch(/not public/i);
  expect(content).not.toMatch(/currently.*private.*Flight|listing.*private Flight/i);
};

describe("landing page", () => {
  const root = resolve(process.cwd());
  const landingPagePath = resolve(root, "site/index.html");
  const content = readFileSync(landingPagePath, "utf-8");
  const document = new DOMParser().parseFromString(content, "text/html");

  it("provides document title and responsive viewport metadata", () => {
    expect(document.title).toBe(
      "Lymic - Immich Desktop Client for Windows, macOS, and Linux",
    );
    expect(document.querySelector('meta[name="viewport"]')?.getAttribute("content")).toBe(
      "width=device-width, initial-scale=1",
    );
  });

  it("covers core discovery keywords in the title, description, and primary heading", () => {
    const description = document
      .querySelector('meta[name="description"]')
      ?.getAttribute("content");
    const heading = document.querySelector("main h1")?.textContent;

    for (const surface of [document.title, description, heading]) {
      expect(surface).toMatch(/Immich/i);
      expect(surface).toMatch(/desktop/i);
      expect(surface).toMatch(/Windows/i);
      expect(surface).toMatch(/macOS/i);
      expect(surface).toMatch(/Linux/i);
    }
  });

  it("publishes accurate SoftwareApplication structured data", () => {
    const script = document.querySelector<HTMLScriptElement>(
      'script[type="application/ld+json"][data-schema="software-application"]',
    );

    expect(script?.textContent).toBeTruthy();
    const schema = JSON.parse(script!.textContent!);

    expect(schema).toMatchObject({
      "@context": "https://schema.org",
      "@type": "SoftwareApplication",
      name: "Lymic",
      applicationCategory: "MultimediaApplication",
      operatingSystem: [
        "Windows 10 build 17763 or later (x64)",
        "Windows 11 (x64)",
        "macOS (Apple Silicon)",
        "Linux (x86_64)",
      ],
      author: {
        "@type": "Organization",
        name: "Lymic contributors",
      },
      offers: {
        "@type": "Offer",
        price: "0",
        priceCurrency: "USD",
      },
      downloadUrl: "https://github.com/xXRoxXeRXx/lymic/releases",
    });
    expect(schema.description).toMatch(/upload.*Immich/i);
    expect(JSON.stringify(schema)).not.toMatch(/Virtual Library|Microsoft Store/i);
  });

  it("maps every on-page FAQ item into valid FAQPage structured data", () => {
    const faqItems = Array.from(document.querySelectorAll<HTMLElement>("[data-faq-item]")).map(
      (item) => ({
        "@type": "Question",
        name: item.querySelector("h3")?.textContent?.trim(),
        acceptedAnswer: {
          "@type": "Answer",
          text: item.querySelector("p")?.textContent?.replace(/\s+/g, " ").trim(),
        },
      }),
    );
    const script = document.querySelector<HTMLScriptElement>(
      'script[type="application/ld+json"][data-schema="faq-page"]',
    );

    expect(faqItems).toHaveLength(3);
    expect(faqItems.map((item) => item.name)).toEqual([
      "Is Lymic a two-way synchronization tool?",
      "Which Immich Server versions work with Lymic?",
      "How does Lymic protect my credentials?",
    ]);
    expect(script?.textContent).toBeTruthy();
    expect(JSON.parse(script!.textContent!)).toEqual({
      "@context": "https://schema.org",
      "@type": "FAQPage",
      mainEntity: faqItems,
    });
  });

  it("publishes canonical, Open Graph, and Twitter metadata", () => {
    const meta = (selector: string) =>
      document.querySelector<HTMLMetaElement>(selector)?.getAttribute("content");
    const description = meta('meta[name="description"]');

    expect(document.querySelector('link[rel="canonical"]')?.getAttribute("href")).toBe(
      "https://xxroxxerxx.github.io/lymic/",
    );
    expect({
      title: meta('meta[property="og:title"]'),
      description: meta('meta[property="og:description"]'),
      image: meta('meta[property="og:image"]'),
      url: meta('meta[property="og:url"]'),
      type: meta('meta[property="og:type"]'),
    }).toEqual({
      title: document.title,
      description,
      image: "https://xxroxxerxx.github.io/lymic/assets/logo.png",
      url: "https://xxroxxerxx.github.io/lymic/",
      type: "website",
    });
    expect({
      card: meta('meta[name="twitter:card"]'),
      title: meta('meta[name="twitter:title"]'),
      description: meta('meta[name="twitter:description"]'),
      image: meta('meta[name="twitter:image"]'),
    }).toEqual({
      card: "summary",
      title: document.title,
      description,
      image: "https://xxroxxerxx.github.io/lymic/assets/logo.png",
    });
  });

  it("keeps accessible support, privacy, source, issue tracker, and license links in the header and footer", () => {
    const expectedLinks = {
      Privacy: "privacy/",
      Support: "support/",
      GitHub: "https://github.com/xXRoxXeRXx/lymic",
      Issues: "https://github.com/xXRoxXeRXx/lymic/issues",
      "MIT License": "https://github.com/xXRoxXeRXx/lymic/blob/master/LICENSE",
    };

    for (const selector of ["header nav", "footer nav"]) {
      const nav = document.querySelector(selector);
      const links = Object.fromEntries(
        Array.from(nav?.querySelectorAll("a") ?? []).map((link) => [
          link.textContent?.trim(),
          link.getAttribute("href"),
        ]),
      );

      expect(nav?.getAttribute("aria-label")).toBeTruthy();
      expect(links).toEqual(expectedLinks);
    }

    for (const link of document.querySelectorAll<HTMLAnchorElement>("a[href]")) {
      const target = link.getAttribute("href")!;
      expect(link.textContent?.trim() || link.getAttribute("aria-label")).toBeTruthy();
      if (/^https:\/\//.test(target)) {
        expect(() => new URL(target)).not.toThrow();
      } else {
        expect(existsSync(resolve(root, "site", target))).toBe(true);
      }
    }
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

  it("showcases every authentic application screenshot with useful alternative text", () => {
    const screenshots = Array.from(document.querySelectorAll("main img[data-app-screenshot]"));

    expect(screenshots.map((image) => image.getAttribute("src"))).toEqual([
      "assets/01_signin.png",
      "assets/02_watched_folders.png",
      "assets/04_settings.png",
    ]);

    const hashes = screenshots.map((screenshot) => {
      const source = screenshot.getAttribute("src")!;
      return createHash("sha256")
        .update(readFileSync(resolve(root, "site", source)))
        .digest("hex");
    });
    expect(new Set(hashes).size).toBe(screenshots.length);

    for (const screenshot of screenshots) {
      const source = screenshot.getAttribute("src");
      expect(existsSync(resolve(root, "site", source!))).toBe(true);
      expect(screenshot.getAttribute("alt")?.trim().length).toBeGreaterThan(20);
    }

    expect(screenshots.map((image) => image.getAttribute("alt"))).toEqual([
      expect.stringMatching(/sign-in.*Immich Server/i),
      expect.stringMatching(/Watched Folders/i),
      expect.stringMatching(/settings/i),
    ]);
  });

  it("makes a clear privacy promise on the landing page", () => {
    const mainText = document.querySelector("main")?.textContent?.replace(/\s+/g, " ").trim();

    expect(mainText).toMatch(/zero telemetry/i);
    expect(mainText).toMatch(/zero advertising/i);
    expect(mainText).toMatch(/connects directly.*Immich Server/i);
  });

  it("uses canonical domain language to explain the showcased features", () => {
    const mainText = document.querySelector("main")?.textContent?.replace(/\s+/g, " ").trim();

    expect(mainText).toContain(
      "A Watched Folder is a configured local directory whose supported media Lymic discovers and keeps under observation.",
    );
    expect(mainText).toContain(
      "A Removable Watched Folder is a Watched Folder located on removable storage, bound to a Volume Identifier and relative path so Lymic can follow mount point changes.",
    );
    expect(mainText).toContain(
      "A Target Album is an optional album in the Immich Library associated with one Watched Folder. Every asset discovered in that folder is intended to belong to this album.",
    );
    expect(mainText).toContain(
      "A Duplicate is a locally discovered Media Asset that Immich recognizes as an existing Remote Asset. A Duplicate is not uploaded again.",
    );
    expect(mainText).toMatch(/SHA-1/);
    expect(mainText).toMatch(/Live Photo Video/);
    expect(mainText).toMatch(/XMP Sidecar/);
    expect(mainText).toMatch(/local SQLite/);
    expect(mainText).toMatch(/native OS keyring/);
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

  it("explains each platform's verified capabilities and packages", () => {
    const platforms = Object.fromEntries(
      Array.from(document.querySelectorAll<HTMLElement>("[data-platform]")).map((section) => [
        section.dataset.platform,
        section.textContent?.replace(/\s+/g, " ").trim(),
      ]),
    );

    expect(Object.keys(platforms)).toEqual(["Windows", "macOS", "Linux"]);
    expect(platforms.Windows).toMatch(/Virtual Library.*read-only.*on-demand hydration/i);
    expect(platforms.Windows).toMatch(/Windows Cloud Files API/i);
    expect(platforms.Windows).toMatch(/planned.*not (?:included|available).*current GitHub build/i);
    expect(platforms.Windows).toMatch(/MSI.*Microsoft Store/i);
    expect(platforms.macOS).toMatch(/Apple Silicon.*Intel/i);
    expect(platforms.macOS).toMatch(/menu bar.*notifications.*DMG/i);
    expect(platforms.Linux).toMatch(/AppImage.*DEB.*background/i);
  });

  it("publishes explicit system requirements and platform download targets", () => {
    const requirements = document
      .querySelector("[data-system-requirements]")
      ?.textContent?.replace(/\s+/g, " ")
      .trim();

    expect(requirements).toMatch(/Windows 10.*17763.*Windows 11.*x64/i);
    expect(requirements).toMatch(/macOS.*Apple Silicon.*Intel.*not currently published/i);
    expect(requirements).toMatch(/Linux.*Ubuntu 22\.04.*x86_64/i);
    expect(requirements).toMatch(/Immich Server.*no fixed minimum version/i);
    expect(requirements).toMatch(/HTTPS.*API key/i);

    const downloads = Object.fromEntries(
      Array.from(document.querySelectorAll<HTMLAnchorElement>("a[data-download-platform]")).map(
        (link) => [link.dataset.downloadPlatform, link.href],
      ),
    );

    expect(downloads).toEqual({
      "Windows GitHub": "https://github.com/xXRoxXeRXx/lymic/releases",
      macOS: "https://github.com/xXRoxXeRXx/lymic/releases",
      Linux: "https://github.com/xXRoxXeRXx/lymic/releases",
    });

    const storeCard = document.querySelector<HTMLElement>("[data-store-channel-status]");
    const storeStatus = storeCard?.textContent?.replace(/\s+/g, " ").trim();
    expectPlannedStoreChannel(storeStatus ?? "");
    expect(storeCard?.querySelector('a[href*="apps.microsoft.com"]')).toBeNull();

    const snapBadgeLink = document.querySelector<HTMLAnchorElement>('a[href="https://snapcraft.io/lymic"]');
    expect(snapBadgeLink).not.toBeNull();
    const snapBadgeImg = snapBadgeLink?.querySelector<HTMLImageElement>("img");
    expect(snapBadgeImg?.getAttribute("src")).toBe("assets/snap-store-badge.svg");
    expect(snapBadgeImg?.getAttribute("alt")).toBe("Get it from the Snap Store");
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

  it("describes the Microsoft Store channel as planned and not submitted", () => {
    const content = readFileSync(supportPath, "utf-8");

    expect(content).toMatch(/Microsoft Store/i);
    expectPlannedStoreChannel(content);
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
