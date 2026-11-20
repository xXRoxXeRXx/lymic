import { describe, expect, it, vi } from "vitest";
import { createRemoteAssetTrash } from "./remote-asset-trash";

describe("Move to Trash without an offered action", () => {
  it("reports every asset as failed and removes nothing when the view has no trashAssets", async () => {
    const removeAsset = vi.fn();
    const trash = createRemoteAssetTrash({ timeline: { removeAsset } });

    const outcome = await trash.trash(["a", "b"]);

    expect(outcome).toEqual({ movedAssetIds: [], failedAssetIds: ["a", "b"], failure: "error" });
    expect(removeAsset).not.toHaveBeenCalled();
  });

  it("uses trashAssets as it is when the view offers it later", async () => {
    let offered: ((assetIds: string[]) => Promise<{ movedAssetIds: string[] }>) | undefined;
    const removeAsset = vi.fn();
    const trash = createRemoteAssetTrash({ timeline: { removeAsset }, get trashAssets() { return offered; } });
    offered = async (assetIds) => ({ movedAssetIds: assetIds });

    const outcome = await trash.trash(["a"]);

    expect(outcome.movedAssetIds).toEqual(["a"]);
    expect(removeAsset).toHaveBeenCalledWith("a");
  });
});
