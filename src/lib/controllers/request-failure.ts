/** How a failed section request (Explore, Archive or Trash Restore) is shown: a missing API key permission, an unreachable or unauthorized server, or anything else. */
export type RequestFailure = "permission_denied" | "unavailable" | "error";

/** The backend error code or message of a rejected request, or "" when there is none. */
export function failureMessage(reason: unknown): string {
  return typeof reason === "string" ? reason : reason instanceof Error ? reason.message : "";
}

/**
 * Classifies a rejected section request by the backend error code, which starts
 * with the section's prefix (for example `places`, `archive` or `trash_restore`).
 * Every section maps the same codes the same way.
 */
export function classifyRequestFailure(reason: unknown, prefix: string): RequestFailure {
  const message = failureMessage(reason);
  if (message.startsWith(`${prefix}_permission_denied:`)) return "permission_denied";
  if (message.startsWith(`${prefix}_authentication_required`) || message.startsWith(`${prefix}_unavailable`)) return "unavailable";
  return "error";
}

/** The text for a failed section request: one message per failure kind. */
export function messageForFailure<T>(failure: RequestFailure, messages: Record<RequestFailure, T>): T {
  return messages[failure];
}
