import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
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
  isRefreshing: false,
  isOffline: false,
  hasInitialLoadError: false,
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
    view.rerender({ timeline: { ...timeline, refresh, isRefreshing: true } });
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

  it("prioritizes an initial load error over the offline banner when no timeline is cached", () => {
    render(TimelineView, {
      timeline: { ...timeline, buckets: [], isOffline: true, hasInitialLoadError: true },
      loadErrorMessage: "Could not load this album",
    });

    expect(screen.getByRole("alert")).toHaveTextContent("Could not load this album");
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("opens a photo in a lightbox with metadata and restores focus after closing", async () => {
    const getAssetDetails = vi.fn().mockResolvedValue({
      originalFileName: "IMG_0001.CR3",
      fNumber: 2.8,
      exposureTime: "1/250",
      iso: 100,
      focalLength: 50,
      lensModel: "RF50mm F1.8 STM",
    });
    render(TimelineView, { timeline, serverUrl: "https://immich.example", onOpenExternalUrl: vi.fn().mockResolvedValue(undefined), getAssetDetails });
    const thumbnail = screen.getByRole("button", { name: "Open photo" });
    thumbnail.focus();
    await fireEvent.click(thumbnail);
    expect(screen.getByRole("dialog", { name: "Media lightbox" })).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "Photo preview" })).toHaveAttribute("src", "lymic-asset://asset/photo-1");
    expect(screen.getByText("Canon EOS R6")).toBeInTheDocument();
    expect(screen.getByText("4,032 × 3,024")).toBeInTheDocument();
    await waitFor(() => expect(screen.getByText("IMG_0001.CR3")).toBeInTheDocument());
    expect(screen.getByText("f/2.8 · 1/250s · ISO 100 · 50mm")).toBeInTheDocument();
    expect(screen.getByText("RF50mm F1.8 STM")).toBeInTheDocument();
    expect(getAssetDetails).toHaveBeenCalledWith("photo-1");
    await fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Media lightbox" })).not.toBeInTheDocument();
    expect(thumbnail).toHaveFocus();
  });

  it("omits unavailable asset details from the lightbox footer", async () => {
    const getAssetDetails = vi.fn().mockResolvedValue({
      originalFileName: null,
      fNumber: null,
      exposureTime: null,
      iso: null,
      focalLength: null,
      lensModel: null,
    });
    render(TimelineView, { timeline, getAssetDetails });

    await fireEvent.click(screen.getByRole("button", { name: "Open photo" }));
    await waitFor(() => expect(getAssetDetails).toHaveBeenCalledWith("photo-1"));

    expect(screen.getByRole("dialog")).not.toHaveTextContent("f/");
    expect(screen.getByRole("dialog")).not.toHaveTextContent("ISO");
  });

  it("navigates with arrows and side icon buttons, renders video playback, and opens the selected asset in Immich", async () => {
    const onOpenExternalUrl = vi.fn().mockResolvedValue(undefined);
    render(TimelineView, { timeline, serverUrl: "https://immich.example/", onOpenExternalUrl });
    await fireEvent.click(screen.getByRole("button", { name: "Open photo" }));
    const nextButton = screen.getByRole("button", { name: "Next asset" });
    const previousButton = screen.getByRole("button", { name: "Previous asset" });
    expect(nextButton).toHaveTextContent("");
    expect(previousButton).toHaveTextContent("");
    await fireEvent.click(nextButton);
    expect(screen.getByLabelText("Video preview")).toHaveAttribute("src", "lymic-asset://video/video-1");
    expect(screen.getByText("1:05")).toBeInTheDocument();
    await fireEvent.click(previousButton);
    expect(screen.getByRole("img", { name: "Photo preview" })).toHaveAttribute("src", "lymic-asset://asset/photo-1");
    await fireEvent.keyDown(window, { key: "ArrowRight" });
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
    expect(target.scrollIntoView).toHaveBeenCalledWith({ behavior: "auto", block: "start" });
  });

  it("marks the visible month as active when the timeline viewport scrolls", async () => {
    render(TimelineView, { timeline });
    const viewport = screen.getByLabelText("Photos");
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
    const viewport = screen.getByLabelText("Photos");
    const target = document.getElementById("timeline-bucket-2024-12-01T00-00-00-000Z")!;
    target.scrollIntoView = vi.fn();
    viewport.getBoundingClientRect = () => ({ top: 0, height: 600 } as DOMRect);
    const thumb = screen.getByRole("button", { name: "Drag timeline scrubber" });
    thumb.setPointerCapture = vi.fn();

    await fireEvent.pointerDown(thumb, { pointerId: 1, clientY: 600 });

    expect(target.scrollIntoView).toHaveBeenCalledWith({ behavior: "auto", block: "start" });
  });

  it("uses Tauri convertFileSrc when present so Windows WebView2 resolves custom protocol URLs", async () => {
    const internals = (globalThis as typeof globalThis & { __TAURI_INTERNALS__?: unknown });
    const previous = internals.__TAURI_INTERNALS__;
    internals.__TAURI_INTERNALS__ = {
      convertFileSrc: (filePath: string, protocol = "asset") =>
        `http://${protocol}.localhost/${encodeURIComponent(filePath)}`,
    };
    try {
      render(TimelineView, { timeline });
      const thumbnailButton = screen.getByRole("button", { name: "Open photo" });
      expect(thumbnailButton.querySelector("img")).toHaveAttribute(
        "src",
        "http://lymic-thumb.localhost/asset/photo-1",
      );
      await fireEvent.click(thumbnailButton);
      expect(screen.getByRole("img", { name: "Photo preview" })).toHaveAttribute(
        "src",
        "http://lymic-asset.localhost/asset/photo-1",
      );
    } finally {
      internals.__TAURI_INTERNALS__ = previous;
    }
  });
});
