import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import "../i18n";
import TimelineView from "./TimelineView.svelte";
import type { TimelineViewState } from "../controllers/timeline-controller.svelte";

vi.stubGlobal("ResizeObserver", class {
  observe() {}
  disconnect() {}
});

vi.stubGlobal("IntersectionObserver", class {
  constructor(private readonly callback: IntersectionObserverCallback) {}
  observe(target: Element) {
    this.callback([{ isIntersecting: true, target } as IntersectionObserverEntry], this as unknown as IntersectionObserver);
  }
  disconnect() {}
});

const timeline: TimelineViewState = {
  buckets: [
    { timeBucket: "2025-03-01T00:00:00.000Z", count: 12 },
    { timeBucket: "2025-02-01T00:00:00.000Z", count: 3 },
    { timeBucket: "2024-12-01T00:00:00.000Z", count: 1 },
  ],
  isLoading: false,
  isOffline: false,
  assetsFor: (timeBucket) => timeBucket === "2025-03-01T00:00:00.000Z" ? [
    { id: "photo-1", width: 4032, height: 3024, type: "IMAGE", takenAt: "2025-03-05T12:34:56.000Z", cameraMake: "Canon", cameraModel: "EOS R6", fileSize: 1234567, duration: null },
    { id: "video-1", width: 1920, height: 1080, type: "VIDEO", takenAt: "2025-03-04T12:34:56.000Z", cameraMake: null, cameraModel: null, fileSize: 2345678, duration: 65 },
  ] : [],
  activateBucket: vi.fn(),
  refresh: vi.fn(),
  clear: vi.fn(),
};

afterEach(cleanup);

describe("TimelineView scrubber", () => {
  it("refreshes the timeline from its header and indicates loading", async () => {
    const refresh = vi.fn();
    const view = render(TimelineView, { timeline: { ...timeline, refresh } });

    await fireEvent.click(screen.getByRole("button", { name: "Refresh timeline" }));

    expect(refresh).toHaveBeenCalledOnce();
    view.rerender({ timeline: { ...timeline, refresh, isLoading: true } });
    expect(screen.getByRole("button", { name: "Refresh timeline" })).toHaveAttribute("aria-busy", "true");
  });

  it("shows an offline banner and retries without hiding cached timeline content", async () => {
    const refresh = vi.fn();
    render(TimelineView, { timeline: { ...timeline, refresh, isOffline: true } });

    await fireEvent.click(screen.getByRole("button", { name: "Retry timeline connection" }));

    expect(screen.getByRole("status")).toHaveTextContent("Timeline is offline");
    expect(screen.getByText("March 2025")).toBeInTheDocument();
    expect(refresh).toHaveBeenCalledOnce();
  });

  it("opens a photo in a lightbox with metadata and restores focus after closing", async () => {
    render(TimelineView, { timeline, serverUrl: "https://immich.example", onOpenExternalUrl: vi.fn().mockResolvedValue(undefined) });
    const thumbnail = screen.getByRole("button", { name: "Open photo" });
    thumbnail.focus();
    await fireEvent.click(thumbnail);
    expect(screen.getByRole("dialog", { name: "Media lightbox" })).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "Photo preview" })).toHaveAttribute("src", "lymic-asset://asset/photo-1");
    expect(screen.getByText("Canon EOS R6")).toBeInTheDocument();
    expect(screen.getByText("4,032 × 3,024")).toBeInTheDocument();
    await fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Media lightbox" })).not.toBeInTheDocument();
    expect(thumbnail).toHaveFocus();
  });

  it("navigates with arrows, renders video playback, and opens the selected asset in Immich", async () => {
    const onOpenExternalUrl = vi.fn().mockResolvedValue(undefined);
    render(TimelineView, { timeline, serverUrl: "https://immich.example/", onOpenExternalUrl });
    await fireEvent.click(screen.getByRole("button", { name: "Open photo" }));
    await fireEvent.keyDown(window, { key: "ArrowRight" });
    expect(screen.getByLabelText("Video preview")).toHaveAttribute("src", "lymic-asset://video/video-1");
    expect(screen.getByText("1:05")).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "Open in Immich" }));
    expect(onOpenExternalUrl).toHaveBeenCalledWith("https://immich.example/photos/video-1");
  });
  it("renders year and month indicators and jumps to a clicked month", async () => {
    render(TimelineView, { timeline });
    const target = document.getElementById("timeline-bucket-2025-02-01T00-00-00-000Z")!;
    target.scrollIntoView = vi.fn();

    await fireEvent.click(screen.getByRole("button", { name: "February 2025" }));

    expect(screen.getByRole("navigation", { name: "Timeline scrubber" })).toHaveTextContent("2025");
    expect(screen.getByRole("navigation", { name: "Timeline scrubber" })).toHaveTextContent("2024");
    expect(target.scrollIntoView).toHaveBeenCalledWith({ behavior: "smooth", block: "start" });
  });

  it("marks the visible month as active when the timeline viewport scrolls", async () => {
    render(TimelineView, { timeline });
    const viewport = screen.getByLabelText("Photo Timeline");
    const march = document.getElementById("timeline-bucket-2025-03-01T00-00-00-000Z")!;
    const february = document.getElementById("timeline-bucket-2025-02-01T00-00-00-000Z")!;
    const december = document.getElementById("timeline-bucket-2024-12-01T00-00-00-000Z")!;
    Object.defineProperty(march, "offsetTop", { value: 0 });
    Object.defineProperty(february, "offsetTop", { value: 400 });
    Object.defineProperty(december, "offsetTop", { value: 800 });
    Object.defineProperty(viewport, "scrollTop", { value: 450, configurable: true });

    await fireEvent.scroll(viewport);

    expect(screen.getByRole("button", { name: "February 2025" })).toHaveAttribute("aria-current", "true");
  });

  it("jumps through periods while the scrubber thumb is dragged", async () => {
    render(TimelineView, { timeline });
    const viewport = screen.getByLabelText("Photo Timeline");
    const target = document.getElementById("timeline-bucket-2024-12-01T00-00-00-000Z")!;
    target.scrollIntoView = vi.fn();
    viewport.getBoundingClientRect = () => ({ top: 0, height: 600 } as DOMRect);
    const thumb = screen.getByRole("button", { name: "Drag timeline scrubber" });
    thumb.setPointerCapture = vi.fn();

    await fireEvent.pointerDown(thumb, { pointerId: 1, clientY: 600 });

    expect(target.scrollIntoView).toHaveBeenCalledWith({ behavior: "smooth", block: "start" });
  });
});
