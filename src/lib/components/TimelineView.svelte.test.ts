import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import "../i18n";
import TimelineView from "./TimelineView.svelte";
import type { TimelineViewState } from "../controllers/timeline-controller.svelte";
import { createFavoritesTimelineController } from "../controllers/favorites-timeline-controller.svelte";
import type { TargetAlbumCatalogue } from "../controllers/target-album-catalogue.svelte";
import type { AssetDetails, TimelineAsset } from "../contracts";

vi.stubGlobal("ResizeObserver", class {
  observe() { }
  disconnect() { }
});

vi.stubGlobal("IntersectionObserver", class {
  constructor(private readonly callback: IntersectionObserverCallback) { }
  observe(target: Element) {
    this.callback([{ isIntersecting: true, target } as IntersectionObserverEntry], this as unknown as IntersectionObserver);
  }
  disconnect() { }
});

const timeline: TimelineViewState = {
  buckets: [
    { timeBucket: "2025-03-01T00:00:00.000Z", count: 12 },
    { timeBucket: "2025-02-01T00:00:00.000Z", count: 3 },
    { timeBucket: "2024-12-01T00:00:00.000Z", count: 1 },
  ],
  refreshGeneration: 0,
  isLoading: false,
  isRefreshing: false,
  isOffline: false,
  hasInitialLoadError: false,
  assetsFor: (timeBucket) => timeBucket === "2025-03-01T00:00:00.000Z" ? [
    { id: "photo-1", width: 4032, height: 3024, type: "IMAGE", takenAt: "2025-03-05T12:34:56.000Z", cameraMake: "Canon", cameraModel: "EOS R6", fileSize: 1234567, duration: null, isFavorite: false },
    { id: "video-1", width: 1920, height: 1080, type: "VIDEO", takenAt: "2025-03-04T12:34:56.000Z", cameraMake: null, cameraModel: null, fileSize: 2345678, duration: 65, isFavorite: true },
  ] : [],
  activateBucket: vi.fn(),
  refresh: vi.fn(),
  clear: vi.fn(),
};

function assetDetails(overrides: Partial<AssetDetails> = {}): AssetDetails {
  return {
    originalFileName: null,
    isFavorite: false,
    cameraMake: null,
    cameraModel: null,
    city: null,
    state: null,
    country: null,
    fNumber: null,
    exposureTime: null,
    iso: null,
    focalLength: null,
    lensModel: null,
    imageWidth: null,
    imageHeight: null,
    fileSize: null,
    ...overrides,
  };
}

function albumCatalogue(albums = [
  { id: "album-summer", albumName: "Summer" },
  { id: "album-work", albumName: "Work" },
]): TargetAlbumCatalogue {
  return {
    albums,
    isLoading: false,
    error: null,
    hasRequested: false,
    refresh: vi.fn().mockResolvedValue(undefined),
    clear: vi.fn(),
    create: vi.fn().mockImplementation(async (name: string) => ({ id: "album-created", albumName: name })),
    nameFor: (id) => albums.find((album) => album.id === id)?.albumName,
  };
}

afterEach(cleanup);

describe("TimelineView scrubber", () => {
  it("navigates into an unloaded favorites month after removing the open asset", async () => {
    const originalObserver = globalThis.IntersectionObserver;
    vi.stubGlobal("IntersectionObserver", class {
      constructor(private readonly callback: IntersectionObserverCallback) {}
      observe(target: Element) {
        if (target.id.includes("2025-03")) this.callback([{ isIntersecting: true, target } as IntersectionObserverEntry], this as unknown as IntersectionObserver);
      }
      disconnect() {}
    });
    try {
      const first = "2025-03-01T00:00:00.000Z";
      const second = "2025-02-01T00:00:00.000Z";
      const asset = (id: string): TimelineAsset => ({ id, width: 1, height: 1, type: "IMAGE", takenAt: null, fileSize: null, duration: null, isFavorite: true });
      const favorites = createFavoritesTimelineController({
        getFavoritesTimelineBuckets: vi.fn().mockResolvedValue([{ timeBucket: first, count: 1 }, { timeBucket: second, count: 1 }]),
        getFavoritesTimelineBucketAssets: vi.fn(async (month: string) => [asset(month === first ? "first" : "second")]),
      });
      await favorites.refresh();
      await favorites.activateBucket(first);
      render(TimelineView, { timeline: favorites, removeUnfavoritedAssets: true, setAssetFavorite: vi.fn().mockResolvedValue(undefined) });
      await fireEvent.click(screen.getByRole("button", { name: "Open photo" }));
      const lightbox = screen.getByRole("dialog", { name: "Media lightbox" });
      await fireEvent.click(within(lightbox).getByRole("button", { name: "Remove from favorites" }));
      await waitFor(() => expect(favorites.buckets).toHaveLength(1));
      await fireEvent.click(within(lightbox).getByRole("button", { name: "Next asset" }));
      await waitFor(() => expect(within(lightbox).getByRole("img", { name: "Photo preview" })).toHaveAttribute("src", "lymic-asset://asset/second"));
    } finally {
      vi.stubGlobal("IntersectionObserver", originalObserver);
    }
  });
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

  it("starts selection from a thumbnail checkbox, prevents opening media, and exits with Escape", async () => {
    render(TimelineView, { timeline });
    const [photoCheckbox] = screen.getAllByRole("checkbox", { name: "Select asset" });

    await fireEvent.click(photoCheckbox);

    expect(screen.getByText("1 selected")).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "Open photo" }));
    expect(screen.queryByRole("dialog", { name: "Media lightbox" })).not.toBeInTheDocument();

    await fireEvent.keyDown(window, { key: "Escape" });

    expect(screen.queryByText("1 selected")).not.toBeInTheDocument();
    expect(photoCheckbox).not.toBeChecked();
  });

  it("downloads one selected asset through the existing original-download path", async () => {
    const getAssetDetails = vi.fn().mockResolvedValue({
      originalFileName: "IMG_0001.CR3",
      cameraMake: null,
      cameraModel: null,
      city: null,
      state: null,
      country: null,
      fNumber: null,
      exposureTime: null,
      iso: null,
      focalLength: null,
      lensModel: null,
      imageWidth: null,
      imageHeight: null,
      fileSize: null,
      isFavorite: false,
    });
    const chooseDownloadDestination = vi.fn().mockResolvedValue("C:/Downloads/IMG_0001.CR3");
    const downloadAsset = vi.fn().mockResolvedValue(undefined);
    render(TimelineView, { timeline, getAssetDetails, chooseDownloadDestination, downloadAsset });

    await fireEvent.click(screen.getAllByRole("checkbox", { name: "Select asset" })[0]);
    await fireEvent.click(screen.getByRole("button", { name: "Download" }));

    await waitFor(() => expect(downloadAsset).toHaveBeenCalledWith("photo-1", "C:/Downloads/IMG_0001.CR3"));
    expect(chooseDownloadDestination).toHaveBeenCalledWith("IMG_0001.CR3");
    expect(screen.getByRole("status")).toHaveTextContent("Downloaded 1 assets");
    expect(screen.getByText("1 selected")).toBeInTheDocument();
  });

  it("reuses cached details when downloading one selected asset", async () => {
    const getAssetDetails = vi.fn().mockResolvedValue({
      originalFileName: "IMG_0001.CR3",
      cameraMake: null,
      cameraModel: null,
      city: null,
      state: null,
      country: null,
      fNumber: null,
      exposureTime: null,
      iso: null,
      focalLength: null,
      lensModel: null,
      imageWidth: null,
      imageHeight: null,
      fileSize: null,
      isFavorite: false,
    });
    const chooseDownloadDestination = vi.fn().mockResolvedValue("C:/Downloads/IMG_0001.CR3");
    const downloadAsset = vi.fn().mockResolvedValue(undefined);
    render(TimelineView, { timeline, getAssetDetails, chooseDownloadDestination, downloadAsset });

    await fireEvent.click(screen.getByRole("button", { name: "Open photo" }));
    await screen.findByRole("button", { name: "Download original" });
    await fireEvent.click(screen.getByRole("button", { name: "Close lightbox" }));
    await fireEvent.click(screen.getAllByRole("checkbox", { name: "Select asset" })[0]);
    await fireEvent.click(screen.getByRole("button", { name: "Download" }));

    await waitFor(() => expect(downloadAsset).toHaveBeenCalledWith("photo-1", "C:/Downloads/IMG_0001.CR3"));
    expect(getAssetDetails).toHaveBeenCalledOnce();
  });

  it("downloads multiple selected assets as an Immich archive", async () => {
    const downloadArchive = vi.fn().mockResolvedValue(undefined);
    render(TimelineView, { timeline, downloadArchive });

    const checkboxes = screen.getAllByRole("checkbox", { name: "Select asset" });
    await fireEvent.click(checkboxes[0]);
    await fireEvent.click(checkboxes[1]);
    await fireEvent.click(screen.getByRole("button", { name: "Download" }));

    await waitFor(() => expect(downloadArchive).toHaveBeenCalledWith(["photo-1", "video-1"]));
    expect(screen.getByRole("status")).toHaveTextContent("Downloaded 2 assets");
    expect(screen.getByText("2 selected")).toBeInTheDocument();
  });

  it("does not start a selected single-asset download when the save dialog is cancelled", async () => {
    const getAssetDetails = vi.fn().mockResolvedValue({ originalFileName: "IMG_0001.CR3" });
    const chooseDownloadDestination = vi.fn().mockResolvedValue(null);
    const downloadAsset = vi.fn();
    render(TimelineView, { timeline, getAssetDetails, chooseDownloadDestination, downloadAsset });

    await fireEvent.click(screen.getAllByRole("checkbox", { name: "Select asset" })[0]);
    await fireEvent.click(screen.getByRole("button", { name: "Download" }));

    await waitFor(() => expect(chooseDownloadDestination).toHaveBeenCalledWith("IMG_0001.CR3"));
    expect(downloadAsset).not.toHaveBeenCalled();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("reports a failed selected archive download", async () => {
    const downloadArchive = vi.fn().mockRejectedValue(new Error("Network unavailable"));
    render(TimelineView, { timeline, downloadArchive });

    const checkboxes = screen.getAllByRole("checkbox", { name: "Select asset" });
    await fireEvent.click(checkboxes[0]);
    await fireEvent.click(checkboxes[1]);
    await fireEvent.click(screen.getByRole("button", { name: "Download" }));

    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Could not download selected assets"));
  });

  it("updates every selected asset as a favorite and ends selection", async () => {
    const setAssetFavorite = vi.fn().mockResolvedValue(undefined);
    render(TimelineView, { timeline, setAssetFavorite });
    const checkboxes = screen.getAllByRole("checkbox", { name: "Select asset" });
    await fireEvent.click(checkboxes[0]);
    await fireEvent.click(checkboxes[1]);

    await fireEvent.click(screen.getByRole("button", { name: "Add to favorites" }));

    await waitFor(() => expect(setAssetFavorite).toHaveBeenCalledTimes(2));
    expect(setAssetFavorite).toHaveBeenNthCalledWith(1, "photo-1", true);
    expect(setAssetFavorite).toHaveBeenNthCalledWith(2, "video-1", true);
    expect(screen.getByRole("status")).toHaveTextContent("Updated 2 assets");
    expect(screen.queryByText("2 selected")).not.toBeInTheDocument();
  });

  it("reports partial favorite updates after ending selection", async () => {
    const setAssetFavorite = vi.fn()
      .mockResolvedValueOnce(undefined)
      .mockRejectedValueOnce(new Error("Network unavailable"));
    render(TimelineView, { timeline, setAssetFavorite });
    const checkboxes = screen.getAllByRole("checkbox", { name: "Select asset" });
    await fireEvent.click(checkboxes[0]);
    await fireEvent.click(checkboxes[1]);

    await fireEvent.click(screen.getByRole("button", { name: "Add to favorites" }));

    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Updated 1 of 2 assets"));
    expect(screen.queryByText("2 selected")).not.toBeInTheDocument();
  });

  it("adds every selected asset to the album chosen in the existing dialog with one batch action", async () => {
    const catalogue = albumCatalogue();
    const addAssetsToAlbum = vi.fn().mockResolvedValue({ succeeded: 2, failed: 0 });
    render(TimelineView, { timeline, albumCatalogue: catalogue, addAssetsToAlbum });
    const checkboxes = screen.getAllByRole("checkbox", { name: "Select asset" });
    await fireEvent.click(checkboxes[0]);
    await fireEvent.click(checkboxes[1]);

    await fireEvent.click(screen.getByRole("button", { name: "Add to album" }));
    const dialog = screen.getByRole("dialog", { name: "Add to album" });
    await fireEvent.click(screen.getByRole("button", { name: "Summer" }));
    await fireEvent.click(within(dialog).getByRole("button", { name: "Add to album" }));

    await waitFor(() => expect(addAssetsToAlbum).toHaveBeenCalledWith(["photo-1", "video-1"], "album-summer"));
    expect(screen.getByRole("status")).toHaveTextContent("Added 2 assets to album");
  });

  it("reports partial batch album assignments", async () => {
    const catalogue = albumCatalogue();
    const addAssetsToAlbum = vi.fn().mockResolvedValue({ succeeded: 1, failed: 1 });
    render(TimelineView, { timeline, albumCatalogue: catalogue, addAssetsToAlbum });
    const checkboxes = screen.getAllByRole("checkbox", { name: "Select asset" });
    await fireEvent.click(checkboxes[0]);
    await fireEvent.click(checkboxes[1]);

    await fireEvent.click(screen.getByRole("button", { name: "Add to album" }));
    const dialog = screen.getByRole("dialog", { name: "Add to album" });
    await fireEvent.click(screen.getByRole("button", { name: "Summer" }));
    await fireEvent.click(within(dialog).getByRole("button", { name: "Add to album" }));

    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Added 1 of 2 assets to album"));
  });

  it("opens a photo in a lightbox with metadata and restores focus after closing", async () => {
    const getAssetDetails = vi.fn().mockResolvedValue({
      originalFileName: "IMG_0001.CR3",
      cameraMake: "Canon",
      cameraModel: "Canon EOS R6",
      city: "Berlin",
      state: "Berlin",
      country: "Germany",
      fNumber: 2.8,
      exposureTime: "1/250",
      iso: 100,
      focalLength: 50,
      lensModel: "RF50mm F1.8 STM",
      imageWidth: 6048,
      imageHeight: 4024,
      fileSize: 12_345_678,
      isFavorite: false,
    });
    render(TimelineView, { timeline, serverUrl: "https://immich.example", onOpenExternalUrl: vi.fn().mockResolvedValue(undefined), getAssetDetails });
    const thumbnail = screen.getByRole("button", { name: "Open photo" });
    thumbnail.focus();
    await fireEvent.click(thumbnail);
    expect(screen.getByRole("dialog", { name: "Media lightbox" })).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "Photo preview" })).toHaveAttribute("src", "lymic-asset://asset/photo-1");
    await fireEvent.click(screen.getByRole("button", { name: "Info" }));
    const infoPanel = screen.getByRole("complementary", { name: "Info" });
    expect(within(infoPanel).getByText("Canon EOS R6")).toBeInTheDocument();
    expect(infoPanel).toHaveTextContent("Berlin, Germany");
    expect(infoPanel).toHaveTextContent("6,048");
    await waitFor(() => expect(within(infoPanel).getByText("IMG_0001.CR3")).toBeInTheDocument());
    expect(within(infoPanel).getByText("f/2.8 · 1/250s · ISO 100 · 50mm")).toBeInTheDocument();
    expect(within(infoPanel).getByText("RF50mm F1.8 STM")).toBeInTheDocument();
    expect(getAssetDetails).toHaveBeenCalledWith("photo-1");
    await fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.getByRole("dialog", { name: "Media lightbox" })).toBeInTheDocument();
    await fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Media lightbox" })).not.toBeInTheDocument();
    expect(thumbnail).toHaveFocus();
  });

  it("saves an original through the chosen native-dialog path and confirms completion", async () => {
    const chooseDownloadDestination = vi.fn().mockResolvedValue("C:/Downloads/IMG_0001.CR3");
    const downloadAsset = vi.fn().mockResolvedValue(undefined);
    const getAssetDetails = vi.fn().mockResolvedValue({
      originalFileName: "IMG_0001.CR3",
      cameraMake: null,
      cameraModel: null,
      city: null,
      state: null,
      country: null,
      fNumber: null,
      exposureTime: null,
      iso: null,
      focalLength: null,
      lensModel: null,
      imageWidth: null,
      imageHeight: null,
      fileSize: null,
      isFavorite: false,
    });
    render(TimelineView, { timeline, getAssetDetails, chooseDownloadDestination, downloadAsset });

    await fireEvent.click(screen.getByRole("button", { name: "Open photo" }));
    const downloadButton = await screen.findByRole("button", { name: "Download original" });
    await fireEvent.click(downloadButton);

    expect(chooseDownloadDestination).toHaveBeenCalledWith("IMG_0001.CR3");
    await waitFor(() => expect(downloadAsset).toHaveBeenCalledWith("photo-1", "C:/Downloads/IMG_0001.CR3"));
    expect(screen.getByRole("status")).toHaveTextContent("Download saved");
  });

  it("assigns the open asset to a searched album and confirms success", async () => {
    const catalogue = albumCatalogue();
    const addAssetsToAlbum = vi.fn().mockResolvedValue({ succeeded: 1, failed: 0 });
    render(TimelineView, { timeline, albumCatalogue: catalogue, addAssetsToAlbum });

    await fireEvent.click(screen.getByRole("button", { name: "Open photo" }));
    await fireEvent.click(screen.getByRole("button", { name: "Add to album" }));

    const dialog = screen.getByRole("dialog", { name: "Add to album" });
    expect(catalogue.refresh).toHaveBeenCalledOnce();
    await fireEvent.input(screen.getByPlaceholderText("Search albums..."), { target: { value: "summer" } });
    expect(dialog).toHaveTextContent("Summer");
    expect(dialog).not.toHaveTextContent("Work");
    await fireEvent.click(screen.getByRole("button", { name: "Summer" }));
    await fireEvent.click(within(dialog).getByRole("button", { name: "Add to album" }));

    await waitFor(() => expect(addAssetsToAlbum).toHaveBeenCalledWith(["photo-1"], "album-summer"));
    expect(screen.queryByRole("dialog", { name: "Add to album" })).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Added to album");
  });

  it("creates and selects a new album before assigning the open asset", async () => {
    const catalogue = albumCatalogue();
    const addAssetsToAlbum = vi.fn().mockResolvedValue({ succeeded: 1, failed: 0 });
    render(TimelineView, { timeline, albumCatalogue: catalogue, addAssetsToAlbum });

    await fireEvent.click(screen.getByRole("button", { name: "Open photo" }));
    await fireEvent.click(screen.getByRole("button", { name: "Add to album" }));
    const dialog = screen.getByRole("dialog", { name: "Add to album" });
    await fireEvent.click(screen.getByRole("button", { name: "Create new album" }));
    await fireEvent.input(screen.getByPlaceholderText("Album name..."), { target: { value: "Holiday" } });
    await fireEvent.click(screen.getByRole("button", { name: "Create" }));
    await fireEvent.click(within(dialog).getByRole("button", { name: "Add to album" }));

    await waitFor(() => expect(catalogue.create).toHaveBeenCalledWith("Holiday"));
    await waitFor(() => expect(addAssetsToAlbum).toHaveBeenCalledWith(["photo-1"], "album-created"));
  });

  it("keeps the album picker open and reports a failed assignment", async () => {
    const catalogue = albumCatalogue();
    const addAssetsToAlbum = vi.fn().mockRejectedValue(new Error("Network unavailable"));
    render(TimelineView, { timeline, albumCatalogue: catalogue, addAssetsToAlbum });

    await fireEvent.click(screen.getByRole("button", { name: "Open photo" }));
    await fireEvent.click(screen.getByRole("button", { name: "Add to album" }));
    const dialog = screen.getByRole("dialog", { name: "Add to album" });
    await fireEvent.click(screen.getByRole("button", { name: "Summer" }));
    await fireEvent.click(within(dialog).getByRole("button", { name: "Add to album" }));

    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Could not add asset to album"));
    expect(screen.getByRole("dialog", { name: "Add to album" })).toBeInTheDocument();
  });

  it("shows download progress and reports a failed original download", async () => {
    let rejectDownload: ((reason?: unknown) => void) | undefined;
    const chooseDownloadDestination = vi.fn().mockResolvedValue("C:/Downloads/IMG_0001.CR3");
    const downloadAsset = vi.fn(() => new Promise<void>((_resolve, reject) => {
      rejectDownload = reject;
    }));
    const getAssetDetails = vi.fn().mockResolvedValue({
      originalFileName: "IMG_0001.CR3",
      cameraMake: null,
      cameraModel: null,
      city: null,
      state: null,
      country: null,
      fNumber: null,
      exposureTime: null,
      iso: null,
      focalLength: null,
      lensModel: null,
      imageWidth: null,
      imageHeight: null,
      fileSize: null,
      isFavorite: false,
    });
    render(TimelineView, { timeline, getAssetDetails, chooseDownloadDestination, downloadAsset });

    await fireEvent.click(screen.getByRole("button", { name: "Open photo" }));
    const downloadButton = await screen.findByRole("button", { name: "Download original" });
    await fireEvent.click(downloadButton);

    expect(downloadButton).toBeDisabled();
    expect(downloadButton.querySelector("svg")).toHaveClass("animate-spin");
    rejectDownload?.(new Error("Network unavailable"));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Could not download original"));
  });

  it("keeps the lightbox open until an in-flight download can report completion", async () => {
    let resolveDownload: (() => void) | undefined;
    const chooseDownloadDestination = vi.fn().mockResolvedValue("C:/Downloads/IMG_0001.CR3");
    const downloadAsset = vi.fn(() => new Promise<void>((resolve) => {
      resolveDownload = resolve;
    }));
    const getAssetDetails = vi.fn().mockResolvedValue({
      originalFileName: "IMG_0001.CR3",
      cameraMake: null,
      cameraModel: null,
      city: null,
      state: null,
      country: null,
      fNumber: null,
      exposureTime: null,
      iso: null,
      focalLength: null,
      lensModel: null,
      imageWidth: null,
      imageHeight: null,
      fileSize: null,
      isFavorite: false,
    });
    render(TimelineView, { timeline, getAssetDetails, chooseDownloadDestination, downloadAsset });

    await fireEvent.click(screen.getByRole("button", { name: "Open photo" }));
    await fireEvent.click(await screen.findByRole("button", { name: "Download original" }));

    expect(screen.getByRole("button", { name: "Next asset" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Close lightbox" })).toBeDisabled();
    await fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.getByRole("dialog", { name: "Media lightbox" })).toBeInTheDocument();

    resolveDownload?.();
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Download saved"));
  });

  it("keeps the lightbox on its current asset while adding it to an album", async () => {
    let resolveAssignment: ((result: { succeeded: number; failed: number }) => void) | undefined;
    const catalogue = albumCatalogue();
    const addAssetsToAlbum = vi.fn(() => new Promise<{ succeeded: number; failed: number }>((resolve) => {
      resolveAssignment = resolve;
    }));
    render(TimelineView, { timeline, albumCatalogue: catalogue, addAssetsToAlbum });

    await fireEvent.click(screen.getByRole("button", { name: "Open photo" }));
    await fireEvent.click(screen.getByRole("button", { name: "Add to album" }));
    const picker = screen.getByRole("dialog", { name: "Add to album" });
    await fireEvent.click(screen.getByRole("button", { name: "Summer" }));
    await fireEvent.click(within(picker).getByRole("button", { name: "Add to album" }));

    await waitFor(() => expect(addAssetsToAlbum).toHaveBeenCalledWith(["photo-1"], "album-summer"));
    expect(screen.getByRole("button", { name: "Close lightbox" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Previous asset" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Next asset" })).toBeDisabled();
    await fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.getByRole("dialog", { name: "Media lightbox" })).toBeInTheDocument();

    resolveAssignment?.({ succeeded: 1, failed: 0 });
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Added to album"));
  });

  it("reports a native save-dialog failure", async () => {
    const chooseDownloadDestination = vi.fn().mockRejectedValue(new Error("Dialog unavailable"));
    const downloadAsset = vi.fn();
    const getAssetDetails = vi.fn().mockResolvedValue({
      originalFileName: "IMG_0001.CR3",
      cameraMake: null,
      cameraModel: null,
      city: null,
      state: null,
      country: null,
      fNumber: null,
      exposureTime: null,
      iso: null,
      focalLength: null,
      lensModel: null,
      imageWidth: null,
      imageHeight: null,
      fileSize: null,
      isFavorite: false,
    });
    render(TimelineView, { timeline, getAssetDetails, chooseDownloadDestination, downloadAsset });

    await fireEvent.click(screen.getByRole("button", { name: "Open photo" }));
    await fireEvent.click(await screen.findByRole("button", { name: "Download original" }));

    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Could not download original"));
    expect(downloadAsset).not.toHaveBeenCalled();
  });

  it("omits unavailable asset details from the lightbox footer", async () => {
    const getAssetDetails = vi.fn().mockResolvedValue({
      originalFileName: null,
      cameraMake: null,
      cameraModel: null,
      city: null,
      state: null,
      country: null,
      fNumber: null,
      exposureTime: null,
      iso: null,
      focalLength: null,
      lensModel: null,
      imageWidth: null,
      imageHeight: null,
      fileSize: null,
      isFavorite: false,
    });
    render(TimelineView, { timeline, getAssetDetails });

    await fireEvent.click(screen.getByRole("button", { name: "Open photo" }));
    await waitFor(() => expect(getAssetDetails).toHaveBeenCalledWith("photo-1"));

    expect(screen.getByRole("dialog")).not.toHaveTextContent("f/");
    expect(screen.getByRole("dialog")).not.toHaveTextContent("ISO");
  });

  it("navigates with arrows and side icon buttons, renders video playback, and opens the selected asset in Immich", async () => {
    const onOpenExternalUrl = vi.fn().mockResolvedValue(undefined);
    const getAssetDetails = vi.fn().mockResolvedValue({
      originalFileName: null,
      cameraMake: null,
      cameraModel: null,
      city: null,
      state: null,
      country: null,
      fNumber: null,
      exposureTime: null,
      iso: null,
      focalLength: null,
      lensModel: null,
      imageWidth: null,
      imageHeight: null,
      fileSize: null,
      isFavorite: false,
    });
    render(TimelineView, { timeline, serverUrl: "https://immich.example/", onOpenExternalUrl, getAssetDetails });
    await fireEvent.click(screen.getByRole("button", { name: "Open photo" }));
    const nextButton = screen.getByRole("button", { name: "Next asset" });
    const previousButton = screen.getByRole("button", { name: "Previous asset" });
    expect(nextButton).toHaveTextContent("");
    expect(previousButton).toHaveTextContent("");
    expect(nextButton).toHaveClass("z-30");
    expect(previousButton).toHaveClass("z-30");
    await fireEvent.click(nextButton);
    expect(screen.getByLabelText("Video preview")).toHaveAttribute("src", "lymic-asset://video/video-1");
    expect(screen.getByText("1:05")).toBeInTheDocument();
    await fireEvent.click(previousButton);
    expect(screen.getByRole("img", { name: "Photo preview" })).toHaveAttribute("src", "lymic-asset://asset/photo-1");
    expect(getAssetDetails).toHaveBeenCalledTimes(2);
    await fireEvent.keyDown(window, { key: "ArrowRight" });
    await fireEvent.click(screen.getByRole("button", { name: "Info" }));
    await fireEvent.click(screen.getByRole("button", { name: "Open in Immich" }));
    expect(onOpenExternalUrl).toHaveBeenCalledWith("https://immich.example/photos/video-1");
  });

  it("updates the favorite icon immediately and syncs the chosen status", async () => {
    let resolveFavorite: (() => void) | undefined;
    const setAssetFavorite = vi.fn(() => new Promise<void>((resolve) => {
      resolveFavorite = resolve;
    }));
    const getAssetDetails = vi.fn().mockResolvedValue({
      originalFileName: null,
      cameraMake: null,
      cameraModel: null,
      city: null,
      state: null,
      country: null,
      fNumber: null,
      exposureTime: null,
      iso: null,
      focalLength: null,
      lensModel: null,
      imageWidth: null,
      imageHeight: null,
      fileSize: null,
      isFavorite: false,
    });
    render(TimelineView, { timeline, getAssetDetails, setAssetFavorite });

    expect(screen.getAllByLabelText("Favorite")).toHaveLength(1);

    await fireEvent.click(screen.getByRole("button", { name: "Open photo" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Add to favorites" })).toBeInTheDocument());
    await fireEvent.click(screen.getByRole("button", { name: "Add to favorites" }));

    expect(screen.getByRole("button", { name: "Remove from favorites" })).toBeInTheDocument();
    expect(screen.getAllByLabelText("Favorite")).toHaveLength(2);
    expect(setAssetFavorite).toHaveBeenCalledWith("photo-1", true);
    resolveFavorite?.();
  });

  it("restores the favorite status and shows an error when updating it fails", async () => {
    const setAssetFavorite = vi.fn().mockRejectedValue(new Error("Network unavailable"));
    const getAssetDetails = vi.fn().mockResolvedValue({
      originalFileName: null,
      cameraMake: null,
      cameraModel: null,
      city: null,
      state: null,
      country: null,
      fNumber: null,
      exposureTime: null,
      iso: null,
      focalLength: null,
      lensModel: null,
      imageWidth: null,
      imageHeight: null,
      fileSize: null,
      isFavorite: false,
    });
    render(TimelineView, { timeline, getAssetDetails, setAssetFavorite });

    await fireEvent.click(screen.getByRole("button", { name: "Open photo" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Add to favorites" })).toBeInTheDocument());
    await fireEvent.click(screen.getByRole("button", { name: "Add to favorites" }));

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Add to favorites" })).toBeInTheDocument();
      expect(screen.getByRole("alert")).toHaveTextContent("Could not update favorite");
    });
  });

  it("keeps another asset interactive and error-free when a previous favorite update fails", async () => {
    let rejectFavorite: ((reason?: unknown) => void) | undefined;
    const setAssetFavorite = vi.fn(() => new Promise<void>((_resolve, reject) => {
      rejectFavorite = reject;
    }));
    const getAssetDetails = vi.fn((assetId: string) => Promise.resolve({
      originalFileName: null,
      cameraMake: null,
      cameraModel: null,
      city: null,
      state: null,
      country: null,
      fNumber: null,
      exposureTime: null,
      iso: null,
      focalLength: null,
      lensModel: null,
      imageWidth: null,
      imageHeight: null,
      fileSize: null,
      isFavorite: assetId === "video-1",
    }));
    render(TimelineView, { timeline, getAssetDetails, setAssetFavorite });

    await fireEvent.click(screen.getByRole("button", { name: "Open photo" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Add to favorites" })).toBeEnabled());
    await fireEvent.click(screen.getByRole("button", { name: "Add to favorites" }));
    await fireEvent.click(screen.getByRole("button", { name: "Next asset" }));

    await waitFor(() => expect(screen.getByRole("button", { name: "Remove from favorites" })).toBeEnabled());
    rejectFavorite?.(new Error("Network unavailable"));

    await waitFor(() => {
      const dialog = screen.getByRole("dialog", { name: "Media lightbox" });
      expect(within(dialog).getByRole("button", { name: "Remove from favorites" })).toBeEnabled();
      expect(within(dialog).queryByRole("alert")).not.toBeInTheDocument();
      expect(screen.getByRole("alert")).toHaveTextContent("Could not update favorite");
    });
  });

  it("restores the last confirmed favorite state without discarding newly loaded details", async () => {
    let rejectRemoval: ((reason?: unknown) => void) | undefined;
    const setAssetFavorite = vi.fn()
      .mockResolvedValueOnce(undefined)
      .mockImplementationOnce(() => new Promise<void>((_resolve, reject) => { rejectRemoval = reject; }));
    const getAssetDetails = vi.fn().mockResolvedValue(assetDetails({ originalFileName: "IMG_0001.CR3" }));
    render(TimelineView, { timeline, getAssetDetails, setAssetFavorite });

    await fireEvent.click(screen.getByRole("button", { name: "Open photo" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Add to favorites" })).toBeEnabled());
    await fireEvent.click(screen.getByRole("button", { name: "Add to favorites" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Remove from favorites" })).toBeEnabled());
    await fireEvent.click(screen.getByRole("button", { name: "Remove from favorites" }));
    rejectRemoval?.(new Error("Network unavailable"));

    await waitFor(() => expect(screen.getByRole("button", { name: "Remove from favorites" })).toBeEnabled());
    await fireEvent.click(screen.getByRole("button", { name: "Info" }));
    expect(screen.getByRole("complementary", { name: "Info" })).toHaveTextContent("IMG_0001.CR3");
  });

  it("keeps the Info drawer open while navigation waits for the newly selected details", async () => {
    let resolvePhoto: ((details: AssetDetails | null) => void) | undefined;
    let resolveVideo: ((details: AssetDetails | null) => void) | undefined;
    const getAssetDetails = vi.fn((assetId: string) => new Promise<AssetDetails | null>((resolve) => {
      if (assetId === "photo-1") resolvePhoto = resolve;
      else resolveVideo = resolve;
    }));
    render(TimelineView, { timeline, getAssetDetails });

    await fireEvent.click(screen.getByRole("button", { name: "Open photo" }));
    await fireEvent.click(screen.getByRole("button", { name: "Info" }));
    expect(screen.getByRole("status")).toHaveTextContent("Loading info");
    await fireEvent.click(screen.getByRole("button", { name: "Next asset" }));
    expect(screen.getByRole("complementary", { name: "Info" })).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Loading info");
    resolvePhoto?.(assetDetails({ originalFileName: "photo.CR3" }));
    await Promise.resolve();
    expect(screen.queryByText("photo.CR3")).not.toBeInTheDocument();
    resolveVideo?.(assetDetails({ originalFileName: "video.mp4" }));
    await waitFor(() => expect(screen.getByRole("complementary", { name: "Info" })).toHaveTextContent("video.mp4"));

    await fireEvent.click(screen.getByRole("button", { name: "Info" }));
    expect(screen.queryByRole("complementary", { name: "Info" })).not.toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "Info" }));
    await fireEvent.click(screen.getByRole("button", { name: "Close info" }));
    expect(screen.queryByRole("complementary", { name: "Info" })).not.toBeInTheDocument();
  });

  it("keeps opening Immich available when the metadata request fails", async () => {
    const onOpenExternalUrl = vi.fn().mockResolvedValue(undefined);
    render(TimelineView, {
      timeline,
      serverUrl: "https://immich.example",
      onOpenExternalUrl,
      getAssetDetails: vi.fn().mockRejectedValue(new Error("Unavailable")),
    });

    await fireEvent.click(screen.getByRole("button", { name: "Open photo" }));
    await fireEvent.click(screen.getByRole("button", { name: "Info" }));
    const openInImmich = await screen.findByRole("button", { name: "Open in Immich" });
    await fireEvent.click(openInImmich);
    expect(onOpenExternalUrl).toHaveBeenCalledWith("https://immich.example/photos/photo-1");
  });

  it("invalidates cached details and favorite overrides after a timeline refresh", async () => {
    const getAssetDetails = vi.fn()
      .mockResolvedValueOnce(assetDetails({ originalFileName: "before-refresh.CR3", isFavorite: true }))
      .mockResolvedValueOnce(assetDetails({ originalFileName: "after-refresh.CR3", isFavorite: false }));
    const view = render(TimelineView, { timeline, getAssetDetails });

    await fireEvent.click(screen.getByRole("button", { name: "Open photo" }));
    await fireEvent.click(screen.getByRole("button", { name: "Info" }));
    await waitFor(() => expect(screen.getByRole("complementary", { name: "Info" })).toHaveTextContent("before-refresh.CR3"));
    view.rerender({ timeline: { ...timeline, refreshGeneration: 1 }, getAssetDetails });

    await waitFor(() => expect(screen.getByRole("complementary", { name: "Info" })).toHaveTextContent("after-refresh.CR3"));
    expect(getAssetDetails).toHaveBeenCalledTimes(2);
  });

  it("keeps the download action stable and disables it until the next details resolve", async () => {
    let resolveVideo: ((details: AssetDetails | null) => void) | undefined;
    const getAssetDetails = vi.fn((assetId: string) => assetId === "photo-1"
      ? Promise.resolve(assetDetails({ originalFileName: "photo.CR3" }))
      : new Promise<AssetDetails | null>((resolve) => { resolveVideo = resolve; }));
    render(TimelineView, { timeline, getAssetDetails });

    await fireEvent.click(screen.getByRole("button", { name: "Open photo" }));
    await screen.findByRole("button", { name: "Download original" });
    await fireEvent.click(screen.getByRole("button", { name: "Next asset" }));

    await waitFor(() => expect(screen.getByRole("button", { name: "Download original" })).toBeDisabled());
    resolveVideo?.(assetDetails());
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

  it("hides manual refresh and retry controls when showRefreshButton is false while preserving error and offline messages", () => {
    render(TimelineView, {
      timeline: {
        ...timeline,
        buckets: [],
        hasInitialLoadError: true,
        isOffline: true,
      },
      showRefreshButton: false,
      loadErrorMessage: "Could not load favorites right now.",
    });

    expect(screen.getByRole("alert")).toHaveTextContent("Could not load favorites right now.");
    expect(screen.queryByRole("button", { name: "Refresh timeline" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /retry/i })).not.toBeInTheDocument();

    cleanup();

    render(TimelineView, {
      timeline: {
        ...timeline,
        isOffline: true,
        hasInitialLoadError: false,
      },
      showRefreshButton: false,
      offlineMessage: "Showing last loaded favorites while offline.",
    });

    expect(screen.getByRole("status")).toHaveTextContent("Showing last loaded favorites while offline.");
    expect(screen.getByText("March 2025")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Refresh timeline" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /retry/i })).not.toBeInTheDocument();
  });

  it("renders a distinct loading status when isLoading is true and loadingMessage is provided", () => {
    render(TimelineView, {
      timeline: {
        ...timeline,
        buckets: [],
        isLoading: true,
        hasInitialLoadError: true,
      },
      loadingMessage: "Loading favorites...",
      emptyMessage: "No favorites yet.",
      loadErrorMessage: "Could not load favorites.",
    });

    expect(screen.getByRole("status")).toHaveTextContent("Loading favorites...");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.queryByText("No favorites yet.")).not.toBeInTheDocument();
  });

  it("uses ariaLabel for the timeline viewport when provided", () => {
    render(TimelineView, {
      timeline,
      ariaLabel: "Favorites",
    });

    expect(screen.getByLabelText("Favorites")).toBeInTheDocument();
  });
});

describe("Favorites removal in TimelineView", () => {
  function favoriteAsset(id: string, takenAt: string, type: "IMAGE" | "VIDEO" = "IMAGE"): TimelineAsset {
    return {
      id,
      width: 1600,
      height: 1200,
      type,
      takenAt,
      cameraMake: null,
      cameraModel: null,
      fileSize: null,
      duration: type === "VIDEO" ? 45 : null,
      isFavorite: true,
    };
  }

  async function renderFavoritesTimeline(
    buckets: Array<{ timeBucket: string; assets: TimelineAsset[] }>,
    options: {
      setAssetFavorite?: (assetId: string, favorite: boolean) => Promise<void>;
      getAssetDetails?: (assetId: string) => Promise<AssetDetails | null>;
    } = {},
  ) {
    const favorites = createFavoritesTimelineController({
      getFavoritesTimelineBuckets: vi.fn().mockResolvedValue(
        buckets.map((entry) => ({ timeBucket: entry.timeBucket, count: entry.assets.length })),
      ),
      getFavoritesTimelineBucketAssets: vi.fn((timeBucket: string) =>
        Promise.resolve(buckets.find((entry) => entry.timeBucket === timeBucket)?.assets ?? []),
      ),
    });
    await favorites.refresh();
    const view = render(TimelineView, {
      timeline: favorites,
      removeUnfavoritedAssets: true,
      showRefreshButton: false,
      ariaLabel: "Favorites",
      emptyMessage: "No favorites yet.",
      getAssetDetails: options.getAssetDetails ?? vi.fn().mockResolvedValue(assetDetails({ isFavorite: true })),
      setAssetFavorite: options.setAssetFavorite ?? vi.fn().mockResolvedValue(undefined),
    });
    const expectedCount = buckets.reduce((sum, entry) => sum + entry.assets.length, 0);
    if (expectedCount > 0) {
      await waitFor(() => expect(screen.getAllByLabelText("Favorite")).toHaveLength(expectedCount));
    }
    return { favorites, view };
  }

  it("keeps a pending favorite removal in the grid and removes it once confirmed while keeping Lightbox open", async () => {
    let resolveFavorite!: () => void;
    const setAssetFavorite = vi.fn(() => new Promise<void>((resolve) => {
      resolveFavorite = resolve;
    }));

    await renderFavoritesTimeline([
      {
        timeBucket: "2025-03-01T00:00:00.000Z",
        assets: [
          favoriteAsset("fav-1", "2025-03-05T12:00:00.000Z"),
          favoriteAsset("fav-2", "2025-03-04T12:00:00.000Z", "VIDEO"),
        ],
      },
      {
        timeBucket: "2025-02-01T00:00:00.000Z",
        assets: [favoriteAsset("fav-3", "2025-02-10T12:00:00.000Z")],
      },
    ], { setAssetFavorite });

    await fireEvent.click(screen.getAllByRole("button", { name: "Open photo" })[0]);
    const lightbox = screen.getByRole("dialog", { name: "Media lightbox" });
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Remove from favorites" }));

    expect(setAssetFavorite).toHaveBeenCalledWith("fav-1", false);
    expect(within(lightbox).getByRole("button", { name: "Add to favorites" })).toBeDisabled();
    expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(2);
    expect(screen.getByText("2 assets")).toBeInTheDocument();

    resolveFavorite();

    await waitFor(() => expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(1));
    expect(screen.queryByText("2 assets")).not.toBeInTheDocument();
    expect(screen.getAllByText("1 assets")).toHaveLength(2);
    expect(screen.getByRole("dialog", { name: "Media lightbox" })).toBeInTheDocument();
    expect(within(lightbox).getByRole("button", { name: "Add to favorites" })).toBeEnabled();
  });

  it("navigates Lightbox from a removed asset's former position across buckets, wraps boundaries, resets on navigation or close, and stays open when none remain", async () => {
    await renderFavoritesTimeline([
      {
        timeBucket: "2025-03-01T00:00:00.000Z",
        assets: [
          favoriteAsset("fav-1", "2025-03-05T12:00:00.000Z"),
          favoriteAsset("fav-2", "2025-03-04T12:00:00.000Z", "VIDEO"),
        ],
      },
      {
        timeBucket: "2025-02-01T00:00:00.000Z",
        assets: [favoriteAsset("fav-3", "2025-02-10T12:00:00.000Z")],
      },
      {
        timeBucket: "2024-12-01T00:00:00.000Z",
        assets: [favoriteAsset("fav-4", "2024-12-10T12:00:00.000Z", "VIDEO")],
      },
    ]);

    // Open fav-2 (index 1, second in March 2025), remove it, then navigate Next across bucket boundary to fav-3
    await fireEvent.click(screen.getAllByRole("button", { name: "Open video" })[0]);
    let lightbox = screen.getByRole("dialog", { name: "Media lightbox" });
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Remove from favorites" }));
    await waitFor(() => expect(within(lightbox).getByRole("button", { name: "Add to favorites" })).toBeEnabled());

    await fireEvent.click(within(lightbox).getByRole("button", { name: "Next asset" }));
    expect(within(lightbox).getByRole("img", { name: "Photo preview" })).toHaveAttribute("src", "lymic-asset://asset/fav-3");

    // Once on an existing asset (fav-3), previous navigates normally to fav-1
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Previous asset" }));
    expect(within(lightbox).getByRole("img", { name: "Photo preview" })).toHaveAttribute("src", "lymic-asset://asset/fav-1");

    // Remove first item (fav-1 -> drops March 2025), Previous wraps cleanly to last item (fav-4)
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Remove from favorites" }));
    await waitFor(() => expect(screen.queryByText("March 2025")).not.toBeInTheDocument());
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Previous asset" }));
    expect(within(lightbox).getByLabelText("Video preview")).toHaveAttribute("src", "lymic-asset://video/fav-4");

    // Remove last item (fav-4 -> drops December 2024), Next wraps cleanly to first remaining item (fav-3)
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Remove from favorites" }));
    await waitFor(() => expect(screen.queryByText("December 2024")).not.toBeInTheDocument());
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Next asset" }));
    expect(within(lightbox).getByRole("img", { name: "Photo preview" })).toHaveAttribute("src", "lymic-asset://asset/fav-3");

    // Remove last remaining favorite (fav-3): empty state renders behind open Lightbox and Next/Previous are safe no-ops
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Remove from favorites" }));
    await waitFor(() => expect(screen.getByText("No favorites yet.")).toBeInTheDocument());
    expect(screen.queryByRole("navigation", { name: "Timeline scrubber" })).not.toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "Media lightbox" })).toBeInTheDocument();

    await fireEvent.click(within(lightbox).getByRole("button", { name: "Next asset" }));
    expect(within(lightbox).getByRole("img", { name: "Photo preview" })).toHaveAttribute("src", "lymic-asset://asset/fav-3");
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Previous asset" }));
    expect(within(lightbox).getByRole("img", { name: "Photo preview" })).toHaveAttribute("src", "lymic-asset://asset/fav-3");

    await fireEvent.click(within(lightbox).getByRole("button", { name: "Close lightbox" }));
    expect(screen.queryByRole("dialog", { name: "Media lightbox" })).not.toBeInTheDocument();
    expect(screen.getByText("No favorites yet.")).toBeInTheDocument();
  });

  it("processes bulk selection removals independently, handles partial and complete failures, and shows the empty state when all remaining favorites are removed", async () => {
    let resolveFirst!: () => void;
    let rejectSecond!: (reason?: unknown) => void;
    const setAssetFavorite = vi.fn()
      .mockImplementationOnce(() => new Promise<void>((resolve) => { resolveFirst = resolve; }))
      .mockImplementationOnce(() => new Promise<void>((_resolve, reject) => { rejectSecond = reject; }))
      .mockRejectedValueOnce(new Error("offline"))
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce(undefined);

    await renderFavoritesTimeline([
      {
        timeBucket: "2025-03-01T00:00:00.000Z",
        assets: [
          favoriteAsset("fav-1", "2025-03-05T12:00:00.000Z"),
          favoriteAsset("fav-2", "2025-03-04T12:00:00.000Z", "VIDEO"),
          favoriteAsset("fav-3", "2025-03-03T12:00:00.000Z"),
        ],
      },
    ], { setAssetFavorite });

    let checkboxes = screen.getAllByRole("checkbox", { name: "Select asset" });
    await fireEvent.click(checkboxes[0]);
    await fireEvent.click(checkboxes[1]);
    await fireEvent.click(screen.getByRole("button", { name: "Remove from favorites" }));

    // While first removal is pending, all 3 assets stay in the grid with Favorite badges
    expect(screen.getAllByLabelText("Favorite")).toHaveLength(3);
    expect(screen.getByText("3 assets")).toBeInTheDocument();

    resolveFirst();
    await waitFor(() => expect(screen.getAllByLabelText("Favorite")).toHaveLength(2));
    expect(screen.getByText("2 assets")).toBeInTheDocument();

    rejectSecond(new Error("offline"));
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Updated 1 of 2 assets"));
    expect(screen.getAllByLabelText("Favorite")).toHaveLength(2);
    expect(screen.getByRole("button", { name: "Open video" })).toBeInTheDocument();
    expect(screen.queryByText(/selected/)).not.toBeInTheDocument();

    // Complete failure retains all remaining favorites and raises an alert
    checkboxes = screen.getAllByRole("checkbox", { name: "Select asset" });
    await fireEvent.click(checkboxes[0]);
    await fireEvent.click(checkboxes[1]);
    await fireEvent.click(screen.getByRole("button", { name: "Remove from favorites" }));

    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Could not update selected assets"));
    expect(screen.getAllByLabelText("Favorite")).toHaveLength(2);
    expect(screen.getByText("2 assets")).toBeInTheDocument();

    // Removing all remaining favorites yields the empty state while still reporting the completion summary
    checkboxes = screen.getAllByRole("checkbox", { name: "Select asset" });
    await fireEvent.click(checkboxes[0]);
    await fireEvent.click(checkboxes[1]);
    await fireEvent.click(screen.getByRole("button", { name: "Remove from favorites" }));

    await waitFor(() => expect(screen.getByText("No favorites yet.")).toBeInTheDocument());
    expect(screen.getByText("Updated 2 assets")).toBeInTheDocument();
    expect(screen.queryByRole("navigation", { name: "Timeline scrubber" })).not.toBeInTheDocument();
  });

  it("rolls back single favorite removal on failure, restores confirmed Favorite status in Lightbox and grid, and shows an error", async () => {
    const setAssetFavorite = vi.fn().mockRejectedValueOnce(new Error("offline"));

    await renderFavoritesTimeline([
      {
        timeBucket: "2025-03-01T00:00:00.000Z",
        assets: [
          favoriteAsset("fav-1", "2025-03-05T12:00:00.000Z"),
          favoriteAsset("fav-2", "2025-03-04T12:00:00.000Z"),
        ],
      },
    ], { setAssetFavorite });

    await fireEvent.click(screen.getAllByRole("button", { name: "Open photo" })[0]);
    const lightbox = screen.getByRole("dialog", { name: "Media lightbox" });
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Remove from favorites" }));

    await waitFor(() => expect(within(lightbox).getByRole("alert")).toHaveTextContent("Could not update favorite"));
    expect(within(lightbox).getByRole("button", { name: "Remove from favorites" })).toBeEnabled();
    expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(2);
    expect(screen.getAllByLabelText("Favorite")).toHaveLength(2);
    expect(screen.getByText("2 assets")).toBeInTheDocument();
  });

  it("navigates Previous from a removed middle asset, resets on close, and keeps Photo Timeline assets when unfavorited", async () => {
    const { view } = await renderFavoritesTimeline([
      {
        timeBucket: "2025-03-01T00:00:00.000Z",
        assets: [
          favoriteAsset("fav-1", "2025-03-05T12:00:00.000Z"),
          favoriteAsset("fav-2", "2025-03-04T12:00:00.000Z", "VIDEO"),
          favoriteAsset("fav-3", "2025-03-03T12:00:00.000Z"),
        ],
      },
    ]);

    await fireEvent.click(screen.getAllByRole("button", { name: "Open video" })[0]);
    let lightbox = screen.getByRole("dialog", { name: "Media lightbox" });
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Remove from favorites" }));
    await waitFor(() => expect(screen.getByText("2 assets")).toBeInTheDocument());

    // Previous from former middle index 1 steps backward to fav-1
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Previous asset" }));
    expect(within(lightbox).getByRole("img", { name: "Photo preview" })).toHaveAttribute("src", "lymic-asset://asset/fav-1");

    // Re-removing fav-1 and closing resets former position so re-opening starts fresh
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Remove from favorites" }));
    await waitFor(() => expect(screen.getByText("1 assets")).toBeInTheDocument());
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Close lightbox" }));
    expect(screen.queryByRole("dialog", { name: "Media lightbox" })).not.toBeInTheDocument();

    // Safeguard: Photo Timeline (default removeUnfavoritedAssets=false) retains unfavorited items
    view.unmount();
    const photoController = createFavoritesTimelineController({
      getFavoritesTimelineBuckets: vi.fn().mockResolvedValue([
        { timeBucket: "2025-03-01T00:00:00.000Z", count: 2 },
      ]),
      getFavoritesTimelineBucketAssets: vi.fn().mockResolvedValue([
        favoriteAsset("photo-1", "2025-03-05T12:00:00.000Z"),
        favoriteAsset("photo-2", "2025-03-04T12:00:00.000Z"),
      ]),
    });
    await photoController.refresh();

    render(TimelineView, {
      timeline: photoController,
      getAssetDetails: vi.fn().mockResolvedValue(assetDetails({ isFavorite: true })),
      setAssetFavorite: vi.fn().mockResolvedValue(undefined),
    });

    await waitFor(() => expect(screen.getAllByLabelText("Favorite")).toHaveLength(2));
    await fireEvent.click(screen.getAllByRole("button", { name: "Open photo" })[0]);
    lightbox = screen.getByRole("dialog", { name: "Media lightbox" });
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Remove from favorites" }));
    await waitFor(() => expect(within(lightbox).getByRole("button", { name: "Add to favorites" })).toBeEnabled());
    expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(2);
    expect(screen.getByText("2 assets")).toBeInTheDocument();
  });

  it("keeps the grid Favorite Heart badge visible while unfavorite confirmation is pending", async () => {
    let resolveFavorite!: () => void;
    const setAssetFavorite = vi.fn(() => new Promise<void>((resolve) => {
      resolveFavorite = resolve;
    }));

    await renderFavoritesTimeline([
      {
        timeBucket: "2025-03-01T00:00:00.000Z",
        assets: [
          favoriteAsset("fav-1", "2025-03-05T12:00:00.000Z"),
          favoriteAsset("fav-2", "2025-03-04T12:00:00.000Z"),
        ],
      },
    ], { setAssetFavorite });

    await fireEvent.click(screen.getAllByRole("button", { name: "Open photo" })[0]);
    const lightbox = screen.getByRole("dialog", { name: "Media lightbox" });
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Remove from favorites" }));

    expect(within(lightbox).getByRole("button", { name: "Add to favorites" })).toBeDisabled();
    expect(screen.getAllByLabelText("Favorite")).toHaveLength(2);

    resolveFavorite();
    await waitFor(() => expect(screen.getAllByLabelText("Favorite")).toHaveLength(1));
  });

  it("restores a just-removed asset into its original bucket slot and navigation order when re-favorited in the same Lightbox session", async () => {
    await renderFavoritesTimeline([
      {
        timeBucket: "2025-03-01T00:00:00.000Z",
        assets: [
          favoriteAsset("fav-1", "2025-03-05T12:00:00.000Z"),
          favoriteAsset("fav-2", "2025-03-04T12:00:00.000Z"),
        ],
      },
      {
        timeBucket: "2025-02-01T00:00:00.000Z",
        assets: [favoriteAsset("fav-3", "2025-02-10T12:00:00.000Z", "VIDEO")],
      },
      {
        timeBucket: "2025-01-01T00:00:00.000Z",
        assets: [favoriteAsset("fav-4", "2025-01-10T12:00:00.000Z")],
      },
    ]);

    await fireEvent.click(screen.getByRole("button", { name: "Open video" }));
    const lightbox = screen.getByRole("dialog", { name: "Media lightbox" });
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Remove from favorites" }));

    await waitFor(() => expect(screen.queryByText("February 2025")).not.toBeInTheDocument());
    expect(screen.getAllByLabelText("Favorite")).toHaveLength(3);

    // Re-favorite fav-3 in the same open Lightbox session
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Add to favorites" }));
    await waitFor(() => expect(screen.getByText("February 2025")).toBeInTheDocument());
    expect(screen.getByRole("button", { name: "Open video" })).toBeInTheDocument();
    expect(screen.getAllByLabelText("Favorite")).toHaveLength(4);

    const bucketHeadings = screen.getAllByRole("heading", { level: 2 }).map((el) => el.textContent);
    expect(bucketHeadings).toEqual(["March 2025", "February 2025", "January 2025"]);

    // Former-position record cleared; navigating Next from restored fav-3 moves cleanly to fav-4 and back to fav-2
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Next asset" }));
    await waitFor(() => expect(within(lightbox).getByRole("img", { name: "Photo preview" })).toHaveAttribute("src", "lymic-asset://asset/fav-4"));

    await fireEvent.click(within(lightbox).getByRole("button", { name: "Previous asset" }));
    await waitFor(() => expect(within(lightbox).queryByRole("img", { name: "Photo preview" })).not.toBeInTheDocument());
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Previous asset" }));
    await waitFor(() => expect(within(lightbox).getByRole("img", { name: "Photo preview" })).toHaveAttribute("src", "lymic-asset://asset/fav-2"));
  });

  it("clears the empty state when the sole favorite is removed and then re-favorited in the same Lightbox session", async () => {
    await renderFavoritesTimeline([
      {
        timeBucket: "2025-03-01T00:00:00.000Z",
        assets: [favoriteAsset("fav-1", "2025-03-05T12:00:00.000Z")],
      },
    ]);

    await fireEvent.click(screen.getByRole("button", { name: "Open photo" }));
    const lightbox = screen.getByRole("dialog", { name: "Media lightbox" });
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Remove from favorites" }));
    await waitFor(() => expect(screen.getByText("No favorites yet.")).toBeInTheDocument());

    await fireEvent.click(within(lightbox).getByRole("button", { name: "Add to favorites" }));
    await waitFor(() => expect(screen.queryByText("No favorites yet.")).not.toBeInTheDocument());
    expect(screen.getByText("March 2025")).toBeInTheDocument();
    expect(screen.getByText("1 assets")).toBeInTheDocument();
    expect(screen.getByLabelText("Favorite")).toBeInTheDocument();
  });

  it("does not let a navigated-away pending removal overwrite removedLightboxPosition", async () => {
    let resolveFirstRemoval!: () => void;
    let calls = 0;
    const setAssetFavorite = vi.fn(() => {
      calls += 1;
      if (calls === 1) {
        return new Promise<void>((resolve) => {
          resolveFirstRemoval = resolve;
        });
      }
      return Promise.resolve();
    });

    await renderFavoritesTimeline([
      {
        timeBucket: "2025-03-01T00:00:00.000Z",
        assets: [
          favoriteAsset("fav-0", "2025-03-06T12:00:00.000Z"),
          favoriteAsset("fav-1", "2025-03-05T12:00:00.000Z", "VIDEO"),
          favoriteAsset("fav-2", "2025-03-04T12:00:00.000Z"),
          favoriteAsset("fav-3", "2025-03-03T12:00:00.000Z"),
        ],
      },
    ], { setAssetFavorite });

    // Open fav-1 (index 1) and start pending removal
    await fireEvent.click(screen.getByRole("button", { name: "Open video" }));
    const lightbox = screen.getByRole("dialog", { name: "Media lightbox" });
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Remove from favorites" }));

    // Navigate to fav-2 while fav-1 removal is still pending, then remove fav-2 immediately
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Next asset" }));
    await waitFor(() => expect(within(lightbox).getByRole("img", { name: "Photo preview" })).toHaveAttribute("src", "lymic-asset://asset/fav-2"));
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Remove from favorites" }));
    await waitFor(() => expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(2));

    // Now fav-1 removal resolves after user has moved on to removed fav-2
    resolveFirstRemoval();
    await waitFor(() => expect(screen.queryByRole("button", { name: "Open video" })).not.toBeInTheDocument());

    // Next from fav-2's former position must still advance to fav-3 (index 1 in [fav-0, fav-3]), not reset to index 0 (fav-0)
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Next asset" }));
    await waitFor(() => expect(within(lightbox).getByRole("img", { name: "Photo preview" })).toHaveAttribute("src", "lymic-asset://asset/fav-3"));
  });

  it("surfaces a dismissible timeline error banner when a favorite update rejects after Lightbox closes without duplicating active alerts", async () => {
    let rejectFavorite!: (error: Error) => void;
    const setAssetFavorite = vi.fn()
      .mockImplementationOnce(() => new Promise<void>((_, reject) => {
        rejectFavorite = reject;
      }))
      .mockRejectedValueOnce(new Error("still offline"))
      .mockResolvedValue(undefined);

    await renderFavoritesTimeline([
      {
        timeBucket: "2025-03-01T00:00:00.000Z",
        assets: [favoriteAsset("fav-1", "2025-03-05T12:00:00.000Z")],
      },
    ], { setAssetFavorite });

    await fireEvent.click(screen.getByRole("button", { name: "Open photo" }));
    let lightbox = screen.getByRole("dialog", { name: "Media lightbox" });
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Remove from favorites" }));

    await fireEvent.click(within(lightbox).getByRole("button", { name: "Close lightbox" }));
    expect(screen.queryByRole("dialog", { name: "Media lightbox" })).not.toBeInTheDocument();

    rejectFavorite(new Error("offline"));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Could not update favorite"));

    await fireEvent.click(screen.getByRole("button", { name: "Dismiss error" }));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();

    // When Lightbox IS still viewing fav-1 on failure, render only Lightbox's alert (never two simultaneous role="alert" nodes)
    await fireEvent.click(screen.getByRole("button", { name: "Open photo" }));
    lightbox = screen.getByRole("dialog", { name: "Media lightbox" });
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Remove from favorites" }));
    await waitFor(() => expect(within(lightbox).getByRole("alert")).toHaveTextContent("Could not update favorite"));
    expect(screen.getAllByRole("alert")).toHaveLength(1);
  });
});
