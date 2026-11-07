import { createHash } from 'node:crypto';
import { createWriteStream, existsSync, readFileSync } from 'node:fs';
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { basename, dirname, join } from 'node:path';
import { Readable } from 'node:stream';
import { pipeline } from 'node:stream/promises';

const repository = 'xXRoxXeRXx/lymic';

function readArguments(argv) {
  const argumentsByName = new Map();

  for (let index = 0; index < argv.length; index += 2) {
    const name = argv[index];
    const value = argv[index + 1];
    if (!name?.startsWith('--') || !value || argumentsByName.has(name)) {
      throw new Error('Usage: node scripts/update-homebrew-cask.mjs --version X.Y.Z [--asset path] [--output path]');
    }
    argumentsByName.set(name, value);
  }

  return argumentsByName;
}

function caskContents(version, checksum) {
  return `cask "lymic" do
  version "${version}"
  sha256 "${checksum}"

  url "https://github.com/${repository}/releases/download/v#{version}/Lymic_#{version}_aarch64.dmg"
  name "Lymic"
  desc "Desktop synchronization client for Immich"
  homepage "https://github.com/${repository}"
  # License: MIT

  depends_on arch: :arm64

  app "Lymic.app"
end
`;
}

async function downloadAsset(version, directory) {
  const fileName = `Lymic_${version}_aarch64.dmg`;
  const url = `https://github.com/${repository}/releases/download/v${version}/${fileName}`;
  const response = await fetch(url);
  if (!response.ok || !response.body) {
    throw new Error(`Could not download ${url}: ${response.status} ${response.statusText}`);
  }

  const assetPath = join(directory, fileName);
  await pipeline(Readable.fromWeb(response.body), createWriteStream(assetPath));
  return assetPath;
}

async function main() {
  const argumentsByName = readArguments(process.argv.slice(2));
  const version = argumentsByName.get('--version');
  const asset = argumentsByName.get('--asset');
  const output = argumentsByName.get('--output') ?? 'packaging/homebrew/lymic.rb';

  if (!version || !/^\d+\.\d+\.\d+$/.test(version)) {
    throw new Error('--version must be a stable semantic version such as 1.2.3');
  }
  if (asset && !existsSync(asset)) {
    throw new Error(`Release asset does not exist: ${asset}`);
  }

  const temporaryDirectory = asset ? undefined : await mkdtemp(join(tmpdir(), 'lymic-homebrew-cask-'));
  try {
    const assetPath = asset ?? await downloadAsset(version, temporaryDirectory);
    const checksum = createHash('sha256').update(readFileSync(assetPath)).digest('hex');
    await mkdir(dirname(output), { recursive: true });
    await writeFile(output, caskContents(version, checksum));
    console.log(`Updated ${output} from ${basename(assetPath)}`);
  } finally {
    if (temporaryDirectory) {
      await rm(temporaryDirectory, { force: true, recursive: true });
    }
  }
}

await main();
