import { describe, expect, it } from "vitest";
import { restoreFeedbackFor, restoreFeedbackMessage, type RemoteAssetRestoreOutcome } from "./remote-asset-restore";

const outcome = (restored: string[], failed: string[], failure: RemoteAssetRestoreOutcome["failure"] = null): RemoteAssetRestoreOutcome => ({
  changedAssetIds: restored,
  failedAssetIds: failed,
  failure,
});

describe("Restore feedback", () => {
  it("picks success, partial, none, permission and failed from the outcome", () => {
    expect(restoreFeedbackFor(outcome(["a", "b"], []), 2)).toEqual({ kind: "success", count: 2 });
    expect(restoreFeedbackFor(outcome(["a"], ["b", "c"]), 3)).toEqual({ kind: "partial", failed: 2, total: 3 });
    expect(restoreFeedbackFor(outcome([], ["a", "b"]), 2)).toEqual({ kind: "none", failed: 2, total: 2 });
    expect(restoreFeedbackFor(outcome([], ["a"], "permission"), 1)).toEqual({ kind: "permission" });
    expect(restoreFeedbackFor(outcome([], ["a"], "error"), 1)).toEqual({ kind: "failed" });
  });

  it("translates each kind with locale-formatted counts", () => {
    const translate = (key: string, options?: { values: Record<string, string | number> }) =>
      `${key}${options ? `:${JSON.stringify(options.values)}` : ""}`;

    expect(restoreFeedbackMessage({ kind: "success", count: 1234 }, translate, "en")).toBe('trash_restore_success:{"count":"1,234"}');
    expect(restoreFeedbackMessage({ kind: "partial", failed: 2, total: 3 }, translate, "en")).toBe('trash_restore_partial:{"failed":"2","count":"3"}');
    expect(restoreFeedbackMessage({ kind: "none", failed: 1, total: 1 }, translate, "en")).toBe('trash_restore_none:{"failed":"1","count":"1"}');
    expect(restoreFeedbackMessage({ kind: "permission" }, translate, "en")).toBe("trash_restore_permission");
    expect(restoreFeedbackMessage({ kind: "failed" }, translate, "en")).toBe("trash_restore_failed");
  });
});
