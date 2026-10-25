export interface ExpectedMsixProperties {
  identityName: string;
  publisher?: string;
  version: string;
  architecture: "x64" | "arm64" | "x86";
}

export interface MsixValidationResult {
  valid: boolean;
  errors: string[];
}

export function toQuadVersion(version: string): string {
  const parts = version.replace(/^v/, "").split(".");
  while (parts.length < 4) {
    parts.push("0");
  }
  return parts.slice(0, 4).join(".");
}

export function validateMsixManifest(
  manifestXml: string,
  expected: ExpectedMsixProperties,
): MsixValidationResult {
  const errors: string[] = [];

  const identityMatch = manifestXml.match(/<Identity\s+([^>]+)\/?>/);
  if (!identityMatch) {
    return { valid: false, errors: ["Missing <Identity> element in AppxManifest.xml"] };
  }

  const identityAttrs = identityMatch[1];
  const nameMatch = identityAttrs.match(/Name="([^"]+)"/);
  const pubMatch = identityAttrs.match(/Publisher="([^"]+)"/);
  const archMatch = identityAttrs.match(/ProcessorArchitecture="([^"]+)"/);
  const versionMatch = identityAttrs.match(/Version="([^"]+)"/);

  const actualName = nameMatch ? nameMatch[1] : "";
  const actualPub = pubMatch ? pubMatch[1] : "";
  const actualArch = archMatch ? archMatch[1] : "";
  const actualVersion = versionMatch ? versionMatch[1] : "";

  if (actualName !== expected.identityName) {
    errors.push(`Identity Name mismatch: expected '${expected.identityName}', got '${actualName}'`);
  }

  if (expected.publisher && actualPub !== expected.publisher) {
    errors.push(`Publisher mismatch: expected '${expected.publisher}', got '${actualPub}'`);
  }

  if (actualArch !== expected.architecture) {
    errors.push(`ProcessorArchitecture mismatch: expected '${expected.architecture}', got '${actualArch}'`);
  }

  const expectedQuadVersion = toQuadVersion(expected.version);
  if (actualVersion !== expectedQuadVersion) {
    errors.push(`Version mismatch: expected '${expectedQuadVersion}', got '${actualVersion}'`);
  }

  if (!manifestXml.includes('Name="runFullTrust"')) {
    errors.push("Missing required capability: 'runFullTrust'");
  }

  if (!manifestXml.includes('Name="broadFileSystemAccess"')) {
    errors.push("Missing required restricted capability: 'broadFileSystemAccess' for watched folders");
  }

  return {
    valid: errors.length === 0,
    errors,
  };
}
