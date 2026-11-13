export function protocolSrc(protocol: "lymic-thumb" | "lymic-asset", kind: "asset" | "video", id: string) {
  const convert = (globalThis as typeof globalThis & {
    __TAURI_INTERNALS__?: { convertFileSrc?: (path: string, protocol?: string) => string };
  }).__TAURI_INTERNALS__?.convertFileSrc;
  const encodedId = encodeURIComponent(id);
  return typeof convert === "function"
    ? `${convert("", protocol).replace(/\/$/, "")}/${kind}/${encodedId}`
    : `${protocol}://${kind}/${encodedId}`;
}
