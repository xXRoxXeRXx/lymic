import type { MapMarker } from "../contracts";

self.onmessage = (event: MessageEvent<MapMarker[]>) => {
  let west = 180;
  let east = -180;
  let south = 90;
  let north = -90;
  const features = event.data.map(({ id, lat, lon }) => {
    west = Math.min(west, lon);
    east = Math.max(east, lon);
    south = Math.min(south, Math.max(-85, lat));
    north = Math.max(north, Math.min(85, lat));
    return {
      type: "Feature" as const,
      properties: { id },
      geometry: { type: "Point" as const, coordinates: [lon, lat] },
    };
  });
  self.postMessage({ data: { type: "FeatureCollection", features }, bounds: [west, south, east, north] });
};
