export function isValidExclusionPattern(pattern: string): boolean {
  return pattern.length > 0
    && !pattern.includes("\\")
    && !pattern.startsWith("/")
    && !pattern.includes("..")
    && !/[?\[\]{}!]/.test(pattern);
}
