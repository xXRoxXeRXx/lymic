import { describe, expect, it } from "vitest";
import { setupBrowserInteractions } from "./prevent-browser-defaults";

describe("setupBrowserInteractions", () => {
  it("prevents the default context menu event", () => {
    const element = document.createElement("div");
    document.body.appendChild(element);

    const cleanup = setupBrowserInteractions(document);

    const event = new MouseEvent("contextmenu", { bubbles: true, cancelable: true });
    element.dispatchEvent(event);

    expect(event.defaultPrevented).toBe(true);

    cleanup();
    document.body.removeChild(element);
  });

  it("prevents dragstart on img elements", () => {
    const img = document.createElement("img");
    document.body.appendChild(img);

    const cleanup = setupBrowserInteractions(document);

    const event = new MouseEvent("dragstart", { bubbles: true, cancelable: true });
    img.dispatchEvent(event);

    expect(event.defaultPrevented).toBe(true);

    cleanup();
    document.body.removeChild(img);
  });

  it("does not prevent dragstart on non-img elements", () => {
    const div = document.createElement("div");
    document.body.appendChild(div);

    const cleanup = setupBrowserInteractions(document);

    const event = new MouseEvent("dragstart", { bubbles: true, cancelable: true });
    div.dispatchEvent(event);

    expect(event.defaultPrevented).toBe(false);

    cleanup();
    document.body.removeChild(div);
  });

  it("prevents Ctrl+P print shortcut", () => {
    const cleanup = setupBrowserInteractions(document);

    const printEvent = new KeyboardEvent("keydown", {
      key: "p",
      ctrlKey: true,
      bubbles: true,
      cancelable: true,
    });
    document.dispatchEvent(printEvent);

    expect(printEvent.defaultPrevented).toBe(true);

    cleanup();
  });

  it("allows standard shortcuts like Ctrl+C or Ctrl+V", () => {
    const cleanup = setupBrowserInteractions(document);

    const copyEvent = new KeyboardEvent("keydown", {
      key: "c",
      ctrlKey: true,
      bubbles: true,
      cancelable: true,
    });
    document.dispatchEvent(copyEvent);

    expect(copyEvent.defaultPrevented).toBe(false);

    const pasteEvent = new KeyboardEvent("keydown", {
      key: "v",
      ctrlKey: true,
      bubbles: true,
      cancelable: true,
    });
    document.dispatchEvent(pasteEvent);

    expect(pasteEvent.defaultPrevented).toBe(false);

    cleanup();
  });
});
