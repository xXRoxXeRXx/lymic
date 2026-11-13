import { expect, it, vi } from "vitest";

it("fits locations on both sides of the antimeridian into a narrow viewport", async () => {
  const postMessage = vi.spyOn(self, "postMessage").mockImplementation(() => {});
  await import("./map-data.worker");
  self.onmessage!(new MessageEvent("message", { data: [{ id: "east", lat: 10, lon: 179 }, { id: "west", lat: 12, lon: -179 }] }));
  expect(postMessage).toHaveBeenCalledWith(expect.objectContaining({ bounds: [179, 10, 181, 12] }));
  postMessage.mockRestore();
});
