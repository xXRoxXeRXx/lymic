/** Mirrors `normalize_patterns` in `src-tauri/src/media/admission.rs` for non-empty drafts. */
export function isValidExclusionPattern(pattern: string): boolean {
  return pattern.length > 0
    && !pattern.includes("\\")
    && !pattern.startsWith("/")
    && !pattern.includes("..")
    && !/[?\[\]{}!]/.test(pattern);
}
