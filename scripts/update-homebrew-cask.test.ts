import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';

const temporaryDirectories: string[] = [];

afterEach(() => {
  for (const directory of temporaryDirectories.splice(0)) {
    rmSync(directory, { force: true, recursive: true });
  }
});

describe('update-homebrew-cask', () => {
  it('writes a Cask whose version, URL, and SHA-256 match the release DMG', () => {
    const directory = mkdtempSync(join(tmpdir(), 'lymic-homebrew-cask-'));
    temporaryDirectories.push(directory);
    const dmgPath = join(directory, 'Lymic_1.2.3_aarch64.dmg');
    const caskPath = join(directory, 'lymic.rb');
    writeFileSync(dmgPath, 'release-dmg-contents');

    execFileSync(process.execPath, [
      'scripts/update-homebrew-cask.mjs',
      '--version',
      '1.2.3',
      '--asset',
      dmgPath,
      '--output',
      caskPath,
    ]);

    expect(readFileSync(caskPath, 'utf8')).toBe(`cask "lymic" do
  version "1.2.3"
  sha256 "dcad419fa0a185c33030b043aca5ab63592e44d1be82bd07bd786fcf080464f8"

  url "https://github.com/xXRoxXeRXx/lymic/releases/download/v#{version}/Lymic_#{version}_aarch64.dmg"
  name "Lymic"
  desc "Desktop synchronization client for Immich"
  homepage "https://github.com/xXRoxXeRXx/lymic"
  # License: MIT

  depends_on arch: :arm64

  app "Lymic.app"
end
`);
  });
});
