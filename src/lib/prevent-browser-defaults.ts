export function setupBrowserInteractions(target: Document | Window = document): () => void {
  const onContextMenu = (event: Event) => {
    event.preventDefault();
  };

  const onDragStart = (event: Event) => {
    const targetElement = event.target as HTMLElement | null;
    if (targetElement && targetElement.tagName === "IMG") {
      event.preventDefault();
    }
  };

  const onKeyDown = (event: Event) => {
    const keyboardEvent = event as KeyboardEvent;
    if ((keyboardEvent.ctrlKey || keyboardEvent.metaKey) && keyboardEvent.key.toLowerCase() === "p") {
      keyboardEvent.preventDefault();
    }
  };

  target.addEventListener("contextmenu", onContextMenu);
  target.addEventListener("dragstart", onDragStart);
  target.addEventListener("keydown", onKeyDown);

  return () => {
    target.removeEventListener("contextmenu", onContextMenu);
    target.removeEventListener("dragstart", onDragStart);
    target.removeEventListener("keydown", onKeyDown);
  };
}
