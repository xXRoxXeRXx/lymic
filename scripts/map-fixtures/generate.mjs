// Deterministic synthetic Immich marker responses for real-WebView profiling.
// Usage: node scripts/map-fixtures/generate.mjs distributed|dense > markers.json
const kind = process.argv[2];
if (!["distributed", "dense"].includes(kind)) throw new Error("Choose distributed or dense");
const count = 300_000;
let seed = 0x163165;
function random() { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed / 0x100000000; }
process.stdout.write("[");
for (let index = 0; index < count; index++) {
  const lat = kind === "dense" ? 52.52 : random() * 170 - 85;
  const lon = kind === "dense" ? 13.405 : random() * 360 - 180;
  process.stdout.write(`${index ? "," : ""}${JSON.stringify({ id: `fixture-${index}`, lat, lon })}`);
}
process.stdout.write("]\n");
