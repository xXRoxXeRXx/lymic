export function createFeedbackController(getUnknownError: () => string) {
  let error = $state("");

  function formatError(value: unknown) {
    if (typeof value === "string") return value;
    if (value instanceof Error) return value.message;
    if (typeof value === "object" && value !== null && "message" in value && typeof value.message === "string") {
      return value.message;
    }
    return getUnknownError();
  }

  return {
    get error() { return error; },
    clear() { error = ""; },
    report(value: unknown) { error = formatError(value); },
    formatError,
  };
}
