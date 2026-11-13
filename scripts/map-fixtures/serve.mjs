// Minimal loopback-only Immich fixture for manual packaged Map View profiling.
import { createServer } from "node:http";
import { createReadStream, statSync, readFileSync } from "node:fs";

const [, , markerPath, videoPath] = process.argv;
if (!markerPath) throw new Error("Usage: node serve.mjs markers.json [video.mp4]");
const markerSize = statSync(markerPath).size;
const video = videoPath ? readFileSync(videoPath) : null;
const png = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/aXcAAAAASUVORK5CYII=", "base64");
const favorites = new Set();
let requests = 0, thumbnailRequests = 0, activeThumbnails = 0, peakThumbnails = 0;
const port = Number(process.env.LYMIC_FIXTURE_PORT || 3001);
if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error("Invalid LYMIC_FIXTURE_PORT");
function json(res, value) { res.writeHead(200, { "content-type": "application/json" }); res.end(JSON.stringify(value)); }
function media(req, res, bytes, contentType) {
  const match = /^bytes=(\d+)-(\d*)$/.exec(req.headers.range || "");
  if (!match) { res.writeHead(200, { "content-type": contentType, "content-length": bytes.length, "accept-ranges": "bytes" }); res.end(bytes); return; }
  const start = Number(match[1]), end = match[2] ? Math.min(Number(match[2]), bytes.length - 1) : bytes.length - 1;
  if (start > end || start >= bytes.length) { res.writeHead(416, { "content-range": `bytes */${bytes.length}` }); res.end(); return; }
  res.writeHead(206, { "content-type": contentType, "content-length": end - start + 1, "content-range": `bytes ${start}-${end}/${bytes.length}`, "accept-ranges": "bytes" });
  res.end(bytes.subarray(start, end + 1));
}
const server = createServer((req, res) => {
  requests++;
  const path = new URL(req.url, "http://localhost").pathname;
  if (req.headers["x-api-key"] !== "fixture-key") { res.writeHead(401); res.end(); return; }
  if (req.method === "GET" && path === "/api/users/me") return json(res, { id: "fixture-user", name: "Fixture User", email: "fixture@example.invalid" });
  if (req.method === "GET" && path === "/api/map/markers") {
    console.log(`${new Date().toISOString()} markers start bytes=${markerSize}`);
    res.writeHead(200, { "content-type": "application/json", "content-length": markerSize });
    const stream = createReadStream(markerPath);
    stream.on("error", (error) => { console.error(error); res.destroy(); });
    res.on("close", () => console.log(`${new Date().toISOString()} markers close complete=${res.writableFinished}`));
    return stream.pipe(res);
  }
  if (req.method === "GET" && ["/api/timeline/buckets", "/api/albums", "/api/people"].includes(path)) return json(res, path.endsWith("people") ? { people: [], total: 0, hidden: 0 } : []);
  const asset = /^\/api\/assets\/(fixture-(\d+))(?:\/(thumbnail|original))?$/.exec(path);
  if (asset) {
    const [, id, index, resource] = asset;
    if (resource === "thumbnail" && req.method === "GET") {
      thumbnailRequests++; activeThumbnails++; peakThumbnails = Math.max(peakThumbnails, activeThumbnails);
      res.on("close", () => { activeThumbnails--; });
      return media(req, res, png, "image/png");
    }
    if (resource === "original" && req.method === "GET") return media(req, res, Number(index) % 2 && video ? video : png, Number(index) % 2 && video ? "video/mp4" : "image/png");
    if (!resource && req.method === "GET") return json(res, { id, type: Number(index) % 2 && video ? "VIDEO" : "IMAGE", originalFileName: Number(index) % 2 && video ? `${id}.mp4` : `${id}.png`, isFavorite: favorites.has(id), exifInfo: { exifImageWidth: 1, exifImageHeight: 1 } });
    if (!resource && req.method === "PUT") {
      let body = "";
      req.on("data", chunk => { body += chunk; if (body.length > 4096) req.destroy(); });
      req.on("end", () => { try { if (JSON.parse(body).isFavorite) favorites.add(id); else favorites.delete(id); json(res, {}); } catch { res.writeHead(400); res.end(); } });
      return;
    }
  }
  console.log(`${new Date().toISOString()} unhandled ${req.method} ${path}`);
  res.writeHead(404); res.end();
});
server.listen(port, "127.0.0.1", () => console.log(`Fixture server http://127.0.0.1:${port} API key fixture-key; video=${Boolean(video)}. Ctrl+C stops it.`));
process.on("SIGINT", () => { console.log(`requests=${requests} thumbnailRequests=${thumbnailRequests} peakConcurrentThumbnailResponses=${peakThumbnails}`); server.close(); });

