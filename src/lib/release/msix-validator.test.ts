import { describe, expect, it } from "vitest";
import { validateMsixManifest } from "./msix-validator";

const SAMPLE_VALID_MANIFEST = `<?xml version="1.0" encoding="utf-8"?>
<Package
  xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
  xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
  xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"
  IgnorableNamespaces="uap rescap">

  <Identity
    Name="xXRoxXeRXx.Lymic"
    Publisher="CN=xXRoxXeRXx"
    Version="0.21.0.0"
    ProcessorArchitecture="x64" />

  <Properties>
    <DisplayName>Lymic</DisplayName>
    <PublisherDisplayName>xXRoxXeRXx</PublisherDisplayName>
    <Logo>Assets\\StoreLogo.png</Logo>
  </Properties>

  <Capabilities>
    <rescap:Capability Name="runFullTrust" />
    <rescap:Capability Name="broadFileSystemAccess" />
  </Capabilities>

  <Applications>
    <Application Id="App" Executable="Lymic.exe" EntryPoint="Windows.FullTrustApplication">
      <uap:VisualElements
        DisplayName="Lymic"
        Description="Lymic desktop sync"
        BackgroundColor="#0f172a"
        Square150x150Logo="Assets\\Square150x150Logo.png"
        Square44x44Logo="Assets\\Square44x44Logo.png" />
    </Application>
  </Applications>
</Package>`;

describe("msix validator", () => {
  it("validates a matching Store MSIX manifest", () => {
    const result = validateMsixManifest(SAMPLE_VALID_MANIFEST, {
      identityName: "xXRoxXeRXx.Lymic",
      version: "0.21.0",
      architecture: "x64",
    });

    expect(result.valid).toBe(true);
    expect(result.errors).toHaveLength(0);
  });

  it("detects wrong identity name", () => {
    const result = validateMsixManifest(SAMPLE_VALID_MANIFEST, {
      identityName: "OtherPublisher.OtherApp",
      version: "0.21.0",
      architecture: "x64",
    });

    expect(result.valid).toBe(false);
    expect(result.errors).toContain(
      "Identity Name mismatch: expected 'OtherPublisher.OtherApp', got 'xXRoxXeRXx.Lymic'",
    );
  });

  it("detects wrong architecture", () => {
    const result = validateMsixManifest(SAMPLE_VALID_MANIFEST, {
      identityName: "xXRoxXeRXx.Lymic",
      version: "0.21.0",
      architecture: "arm64",
    });

    expect(result.valid).toBe(false);
    expect(result.errors).toContain(
      "ProcessorArchitecture mismatch: expected 'arm64', got 'x64'",
    );
  });

  it("detects wrong version", () => {
    const result = validateMsixManifest(SAMPLE_VALID_MANIFEST, {
      identityName: "xXRoxXeRXx.Lymic",
      version: "0.22.0",
      architecture: "x64",
    });

    expect(result.valid).toBe(false);
    expect(result.errors).toContain(
      "Version mismatch: expected '0.22.0.0', got '0.21.0.0'",
    );
  });

  it("detects missing required restricted capabilities", () => {
    const manifestWithoutCap = SAMPLE_VALID_MANIFEST.replace(
      '<rescap:Capability Name="broadFileSystemAccess" />',
      "",
    );
    const result = validateMsixManifest(manifestWithoutCap, {
      identityName: "xXRoxXeRXx.Lymic",
      version: "0.21.0",
      architecture: "x64",
    });

    expect(result.valid).toBe(false);
    expect(result.errors.some((e) => e.includes("broadFileSystemAccess"))).toBe(true);
  });
});
