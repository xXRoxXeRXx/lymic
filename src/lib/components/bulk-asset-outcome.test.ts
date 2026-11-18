import { describe, expect, it } from "vitest";
import {
  bulkAssetFeedbackFor,
  bulkAssetFeedbackMessage,
  bulkAssetOutcomeFromFailure,
  bulkAssetOutcomeFromResult,
  type BulkAssetOutcome,
} from "./bulk-asset-outcome";

const outcome = (changed: string[], failed: string[], failure: BulkAssetOutcome["failure"] = null): BulkAssetOutcome => ({
  changedAssetIds: changed,
  failedAssetIds: failed,
  failure,
});

const keys = {
  success: "x_success",
  partial: "x_partial",
  none: "x_none",
  permission: "x_permission",
  failed: "x_failed",
};

describe("bulk Remote Asset write outcome", () => {
  it("counts only requested assets as changed and everything else as failed", () => {
    expect(bulkAssetOutcomeFromResult(["a", "b", "c"], ["c", "a", "zzz"])).toEqual(outcome(["c", "a"], ["b"]));
    expect(bulkAssetOutcomeFromResult(["a"], [])).toEqual(outcome([], ["a"]));
  });

  it("maps a request-level failure to permission or error with every asset failed", () => {
    expect(bulkAssetOutcomeFromFailure(["a", "b"], new Error("x_permission_denied:asset.delete"), "x")).toEqual(outcome([], ["a", "b"], "permission"));
    expect(bulkAssetOutcomeFromFailure(["a"], "x_unavailable", "x")).toEqual(outcome([], ["a"], "error"));
    expect(bulkAssetOutcomeFromFailure(["a"], new Error("boom"), "x")).toEqual(outcome([], ["a"], "error"));
  });
});

describe("bulk Remote Asset feedback", () => {
  it("picks success, partial, none, permission and failed from the outcome", () => {
    expect(bulkAssetFeedbackFor(outcome(["a", "b"], []), 2)).toEqual({ kind: "success", count: 2 });
    expect(bulkAssetFeedbackFor(outcome(["a"], ["b", "c"]), 3)).toEqual({ kind: "partial", failed: 2, total: 3 });
    expect(bulkAssetFeedbackFor(outcome([], ["a", "b"]), 2)).toEqual({ kind: "none", failed: 2, total: 2 });
    expect(bulkAssetFeedbackFor(outcome([], ["a"], "permission"), 1)).toEqual({ kind: "permission" });
    expect(bulkAssetFeedbackFor(outcome([], ["a"], "error"), 1)).toEqual({ kind: "failed" });
  });

  it("translates with the keys the action supplies and locale-formatted counts", () => {
    const translate = (key: string, options?: { values: Record<string, string | number> }) =>
      `${key}${options ? `:${JSON.stringify(options.values)}` : ""}`;

    expect(bulkAssetFeedbackMessage({ kind: "success", count: 1234 }, keys, translate, "en")).toBe('x_success:{"count":"1,234"}');
    expect(bulkAssetFeedbackMessage({ kind: "partial", failed: 2, total: 3 }, keys, translate, "en")).toBe('x_partial:{"failed":"2","count":"3"}');
    expect(bulkAssetFeedbackMessage({ kind: "none", failed: 1, total: 1 }, keys, translate, "en")).toBe('x_none:{"failed":"1","count":"1"}');
    expect(bulkAssetFeedbackMessage({ kind: "permission" }, keys, translate, "en")).toBe("x_permission");
    expect(bulkAssetFeedbackMessage({ kind: "failed" }, keys, translate, "en")).toBe("x_failed");
  });
});
