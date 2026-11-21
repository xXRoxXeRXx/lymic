import { describe, expect, it, vi } from "vitest";
import type { TrashMoveResult } from "../contracts";
import { createRemoteAssetTrash, trashFeedbackFor } from "./remote-asset-trash";

describe("Move to Trash without an offered action", () => {
  it("reports every asset as failed and removes nothing when the view has no trashAssets", async () => {
    const removeAsset = vi.fn();
    const trash = createRemoteAssetTrash({ timeline: { removeAsset } });

    const outcome = await trash.trash(["a", "b"]);

    expect(outcome).toEqual({ movedAssetIds: [], failedAssetIds: ["a", "b"], failure: "error" });
    expect(removeAsset).not.toHaveBeenCalled();
  });

  it("uses trashAssets as it is when the view offers it later", async () => {
    let offered: ((assetIds: string[]) => Promise<TrashMoveResult>) | undefined;
    const removeAsset = vi.fn();
    const trash = createRemoteAssetTrash({ timeline: { removeAsset }, get trashAssets() { return offered; } });
    offered = async (assetIds) => ({ movedAssetIds: assetIds });

    const outcome = await trash.trash(["a"]);

    expect(outcome.movedAssetIds).toEqual(["a"]);
    expect(removeAsset).toHaveBeenCalledWith("a");
  });
});

describe("Move to Trash that stopped after some assets were moved", () => {
  async function stoppedWith(failureCode: string) {
    const removeAsset = vi.fn();
    const onRemoved = vi.fn();
    const trash = createRemoteAssetTrash({
      timeline: { removeAsset },
      onRemoved,
      trashAssets: async () => ({ movedAssetIds: ["a", "b"], failureCode }),
    });
    const outcome = await trash.trash(["a", "b", "c", "d"]);
    return { outcome, removeAsset, onRemoved };
  }

  it("removes the moved assets, keeps the others and reports the refusal", async () => {
    const { outcome, removeAsset, onRemoved } = await stoppedWith("trash_move_permission_denied:asset.delete");

    expect(outcome).toEqual({ movedAssetIds: ["a", "b"], failedAssetIds: ["c", "d"], failure: "permission" });
    expect(removeAsset.mock.calls).toEqual([["a"], ["b"]]);
    expect(onRemoved.mock.calls).toEqual([["a"], ["b"]]);
    expect(trashFeedbackFor(outcome, 4)).toEqual({ kind: "permission" });
  });

  it.each(["trash_move_authentication_required", "trash_move_unavailable", "something unexpected"])("reports any other stop (%s) as a failed request", async (failureCode) => {
    const { outcome, removeAsset } = await stoppedWith(failureCode);

    expect(outcome).toEqual({ movedAssetIds: ["a", "b"], failedAssetIds: ["c", "d"], failure: "error" });
    expect(removeAsset).toHaveBeenCalledTimes(2);
    expect(trashFeedbackFor(outcome, 4)).toEqual({ kind: "failed" });
  });

  it("reports a complete move without a failure", async () => {
    const trash = createRemoteAssetTrash({ timeline: { removeAsset: vi.fn() }, trashAssets: async (ids) => ({ movedAssetIds: ids }) });

    expect((await trash.trash(["a", "b"])).failure).toBeNull();
  });

  it("still fails every asset when the request is rejected before any asset was moved", async () => {
    const removeAsset = vi.fn();
    const trash = createRemoteAssetTrash({
      timeline: { removeAsset },
      trashAssets: async () => { throw "trash_move_permission_denied:asset.delete"; },
    });

    expect(await trash.trash(["a", "b"])).toEqual({ movedAssetIds: [], failedAssetIds: ["a", "b"], failure: "permission" });
    expect(removeAsset).not.toHaveBeenCalled();
  });
});
