// Deterministic Immich marker responses for packaged-WebView profiling.
// Usage: node scripts/map-fixtures/generate.mjs distributed|regional|identical > markers.json
import { once } from "node:events";

const kind = process.argv[2];
if (!["distributed", "regional", "identical", "dense"].includes(kind)) {
  throw new Error("Choose distributed, regional, or identical (dense is an alias for identical)");
}
const count = 300_000;
let seed = 0x163165;
function random() { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed / 0x100000000; }
async function write(chunk) {
  if (!process.stdout.write(chunk)) await once(process.stdout, "drain");
}
await write("[");
for (let index = 0; index < count; index++) {
  let lat;
  let lon;
  if (kind === "identical" || kind === "dense") {
    lat = 52.52; lon = 13.405;
  } else if (kind === "regional") {
    lat = 52.42 + random() * 0.2; lon = 13.305 + random() * 0.2;
  } else {
    lat = random() * 170 - 85; lon = random() * 360 - 180;
    // Explicit edge locations are valid and included in the total count.
    if (index === 0) { lat = 0; lon = 0; }
    if (index === 1) { lat = 84.9; lon = 179.99; }
    if (index === 2) { lat = 84.9; lon = -179.99; }
    if (index === 3) { lat = -89; lon = 45; }
  }
  await write(`${index ? "," : ""}${JSON.stringify({ id: `fixture-${index}`, lat, lon })}`);
}
await write("]\n");
