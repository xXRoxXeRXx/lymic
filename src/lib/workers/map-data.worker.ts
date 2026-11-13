import type { MapMarker } from "../contracts";

self.onmessage = (event: MessageEvent<MapMarker[]>) => {
  let west = 180;
  let east = -180;
  let south = 90;
  let north = -90;
  const longitudes: number[] = [];
  const features = event.data.map(({ id, lat, lon }) => {
    longitudes.push(lon);
    south = Math.min(south, Math.max(-85, lat));
    north = Math.max(north, Math.min(85, lat));
    return {
      type: "Feature" as const,
      properties: { id },
      geometry: { type: "Point" as const, coordinates: [lon, lat] },
    };
  });
  if (longitudes.length) {
    longitudes.sort((a, b) => a - b);
    let largestGap = longitudes[0] + 360 - longitudes[longitudes.length - 1];
    let start = 0;
    for (let i = 1; i < longitudes.length; i++) {
      const gap = longitudes[i] - longitudes[i - 1];
      if (gap > largestGap) { largestGap = gap; start = i; }
    }
    west = longitudes[start];
    east = longitudes[(start + longitudes.length - 1) % longitudes.length];
    if (east < west) east += 360;
  }
  self.postMessage({ data: { type: "FeatureCollection", features }, bounds: [west, south, east, north] });
};
