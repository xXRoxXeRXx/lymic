import { classifyRequestFailure } from "../controllers/request-failure";

/** What a bulk write to Remote Assets did. `failure` is set only when the whole request failed; a partial result leaves it null. */
export interface BulkAssetOutcome {
  /** Remote Assets the write confirmed as changed. */
  changedAssetIds: string[];
  /** Requested Remote Assets not confirmed as changed; every requested asset when the request failed. */
  failedAssetIds: string[];
  /** `permission` when Immich refused the write (HTTP 403), `error` for any other failed request. */
  failure: "permission" | "error" | null;
}

/** What the user is told about a bulk write to `total` Remote Assets. */
export type BulkAssetFeedback =
  | { kind: "success"; count: number }
  | { kind: "partial"; failed: number; total: number }
  | { kind: "none"; failed: number; total: number }
  | { kind: "failed" }
  | { kind: "permission" };

/** The locale key the action uses for each kind of feedback. */
export interface BulkAssetFeedbackKeys {
  success: string;
  partial: string;
  none: string;
  permission: string;
  failed: string;
}

type Translate = (key: string, options?: { values: Record<string, string | number> }) => string;

/** The outcome of a write that answered: requested assets it names are changed, every other requested asset failed. */
export function bulkAssetOutcomeFromResult(requestedAssetIds: string[], changedAssetIds: string[]): BulkAssetOutcome {
  const requested = new Set(requestedAssetIds);
  const changed = changedAssetIds.filter((assetId) => requested.has(assetId));
  const confirmed = new Set(changed);
  return { changedAssetIds: changed, failedAssetIds: requestedAssetIds.filter((assetId) => !confirmed.has(assetId)), failure: null };
}

/**
 * The outcome of a write whose request failed. The backend reports a refusal as a credential-free code that
 * starts with `codePrefix`, so the error text is never shown.
 */
export function bulkAssetOutcomeFromFailure(requestedAssetIds: string[], error: unknown, codePrefix: string): BulkAssetOutcome {
  const refused = classifyRequestFailure(error, codePrefix) === "permission_denied";
  return { changedAssetIds: [], failedAssetIds: [...requestedAssetIds], failure: refused ? "permission" : "error" };
}

/** Maps an outcome to its feedback, the one place that decides success, partial, none, permission or failed. */
export function bulkAssetFeedbackFor(outcome: BulkAssetOutcome, total: number): BulkAssetFeedback {
  if (outcome.failure === "permission") return { kind: "permission" };
  if (outcome.failure === "error") return { kind: "failed" };
  if (outcome.failedAssetIds.length === 0) return { kind: "success", count: outcome.changedAssetIds.length };
  if (outcome.changedAssetIds.length === 0) return { kind: "none", failed: outcome.failedAssetIds.length, total };
  return { kind: "partial", failed: outcome.failedAssetIds.length, total };
}

/** The localized message for bulk feedback, with the action's own `keys`; counts are formatted for `numberLocale`. */
export function bulkAssetFeedbackMessage(feedback: BulkAssetFeedback, keys: BulkAssetFeedbackKeys, translate: Translate, numberLocale: string): string {
  const count = (value: number) => value.toLocaleString(numberLocale);
  switch (feedback.kind) {
    case "success": return translate(keys.success, { values: { count: count(feedback.count) } });
    case "partial": return translate(keys.partial, { values: { failed: count(feedback.failed), count: count(feedback.total) } });
    case "none": return translate(keys.none, { values: { failed: count(feedback.failed), count: count(feedback.total) } });
    case "permission": return translate(keys.permission);
    case "failed": return translate(keys.failed);
  }
}
