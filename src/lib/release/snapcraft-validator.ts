export interface ExpectedSnapcraftProperties {
  name: string;
  version?: string;
  confinement?: "strict" | "classic" | "devmode";
  base?: "core22" | "core24";
}

export interface SnapcraftValidationResult {
  valid: boolean;
  errors: string[];
}

export function validateSnapcraftConfig(
  yamlContent: string,
  expected: ExpectedSnapcraftProperties,
): SnapcraftValidationResult {
  const errors: string[] = [];

  const nameMatch = yamlContent.match(/^name:\s*['"]?([a-zA-Z0-9_-]+)['"]?/m);
  const actualName = nameMatch ? nameMatch[1] : "";
  if (actualName !== expected.name) {
    errors.push(`Snap name mismatch: expected '${expected.name}', got '${actualName}'`);
  }

  const baseMatch = yamlContent.match(/^base:\s*['"]?([a-zA-Z0-9_-]+)['"]?/m);
  const actualBase = baseMatch ? baseMatch[1] : "";
  if (expected.base && actualBase !== expected.base) {
    errors.push(`Base mismatch: expected '${expected.base}', got '${actualBase}'`);
  }

  const confinementMatch = yamlContent.match(/^confinement:\s*['"]?([a-zA-Z0-9_-]+)['"]?/m);
  const actualConfinement = confinementMatch ? confinementMatch[1] : "";
  if (expected.confinement && actualConfinement !== expected.confinement) {
    errors.push(
      `Confinement mismatch: expected '${expected.confinement}', got '${actualConfinement}'`,
    );
  }

  if (expected.version) {
    const versionMatch = yamlContent.match(/^version:\s*['"]?([^'"]+)['"]?/m);
    const actualVersion = versionMatch ? versionMatch[1] : "";
    if (actualVersion !== expected.version) {
      errors.push(`Version mismatch: expected '${expected.version}', got '${actualVersion}'`);
    }
  }

  // Extension check: GNOME extension
  const hasGnomeExtension =
    /extensions:\s*\[[^\]]*gnome[^\]]*\]/m.test(yamlContent) ||
    /extensions:\s*\n\s*-\s*gnome/m.test(yamlContent);
  if (!hasGnomeExtension) {
    errors.push(`Missing required extension: 'gnome' under apps.${expected.name}`);
  }

  // Plugs checks
  const requiredPlugs = ["network", "home", "removable-media", "password-manager-service"];
  for (const plug of requiredPlugs) {
    const plugRegex = new RegExp(`-\\s*${plug}\\b`);
    if (!plugRegex.test(yamlContent)) {
      errors.push(`Missing required plug: '${plug}'`);
    }
  }

  // WebKitGTK layout check
  const hasLayoutSection = /^layout:/m.test(yamlContent);
  const hasWebkitLayout = /webkit2gtk-4\.1/m.test(yamlContent);
  if (!hasLayoutSection || !hasWebkitLayout) {
    errors.push("Missing required layout mapping for webkit2gtk-4.1");
  }

  // Store metadata check
  if (!/^title:\s*['"]?Lymic['"]?/m.test(yamlContent)) {
    errors.push("Missing or invalid store title declaration: expected 'title: Lymic'");
  }
  if (!/^icon:\s*['"]?[^'"\r\n]+['"]?/m.test(yamlContent)) {
    errors.push("Missing store icon declaration: expected 'icon: snap/gui/icon.png'");
  }
  if (!/^license:\s*['"]?MIT['"]?/m.test(yamlContent)) {
    errors.push("Missing or invalid license declaration: expected 'license: MIT'");
  }
  if (!/^website:\s*https?:\/\//m.test(yamlContent)) {
    errors.push("Missing store website URL");
  }

  return {
    valid: errors.length === 0,
    errors,
  };
}

