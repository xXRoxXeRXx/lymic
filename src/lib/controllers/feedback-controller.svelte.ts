export function formatError(value: unknown, fallback = "Unknown error") {
  if (typeof value === "string") return value;
  if (value instanceof Error) return value.message;
  if (typeof value === "object" && value !== null && "message" in value && typeof value.message === "string") {
    return value.message;
  }
  return fallback;
}

export function createFeedbackController(getUnknownError: () => string) {
  let error = $state("");

  return {
    get error() { return error; },
    clear() { error = ""; },
    report(value: unknown) { error = formatError(value, getUnknownError()); },
    formatError(value: unknown) { return formatError(value, getUnknownError()); },
  };
}
