/** How a failed Explore load is shown: a missing API key permission, an unreachable or unauthorized server, or anything else. */
export type ExploreError = "permission_denied" | "unavailable" | "error";

/**
 * Classifies a rejected Explore load by the backend error code, which starts
 * with the section's prefix (for example `places` or `recognized_people`).
 * Every section maps the same codes the same way.
 */
export function classifyExploreError(reason: unknown, prefix: string): ExploreError {
  const message = typeof reason === "string" ? reason : reason instanceof Error ? reason.message : "";
  if (message.startsWith(`${prefix}_permission_denied:`)) return "permission_denied";
  if (message.startsWith(`${prefix}_authentication_required`) || message.startsWith(`${prefix}_unavailable`)) return "unavailable";
  return "error";
}
