import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import "../i18n";
import TimelineView from "./TimelineView.svelte";
import type { TimelineViewState } from "../controllers/timeline-controller.svelte";
import type { TargetAlbumCatalogue } from "../controllers/target-album-catalogue.svelte";
import type { AssetDetails } from "../contracts";

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
    const addAssetToAlbum = vi.fn().mockResolvedValue(undefined);
    render(TimelineView, { timeline, albumCatalogue: catalogue, addAssetToAlbum });

    await fireEvent.click(screen.getByRole("button", { name: "Open photo" }));
    await fireEvent.click(screen.getByRole("button", { name: "Add to album" }));

    const dialog = screen.getByRole("dialog", { name: "Add to album" });
    expect(catalogue.refresh).toHaveBeenCalledOnce();
    await fireEvent.input(screen.getByPlaceholderText("Search albums..."), { target: { value: "summer" } });
    expect(dialog).toHaveTextContent("Summer");
    expect(dialog).not.toHaveTextContent("Work");
    await fireEvent.click(screen.getByRole("button", { name: "Summer" }));
    await fireEvent.click(within(dialog).getByRole("button", { name: "Add to album" }));

    await waitFor(() => expect(addAssetToAlbum).toHaveBeenCalledWith("photo-1", "album-summer"));
    expect(screen.queryByRole("dialog", { name: "Add to album" })).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Added to album");
  });

  it("creates and selects a new album before assigning the open asset", async () => {
    const catalogue = albumCatalogue();
    const addAssetToAlbum = vi.fn().mockResolvedValue(undefined);
    render(TimelineView, { timeline, albumCatalogue: catalogue, addAssetToAlbum });

    await fireEvent.click(screen.getByRole("button", { name: "Open photo" }));
    await fireEvent.click(screen.getByRole("button", { name: "Add to album" }));
    const dialog = screen.getByRole("dialog", { name: "Add to album" });
    await fireEvent.click(screen.getByRole("button", { name: "Create new album" }));
    await fireEvent.input(screen.getByPlaceholderText("Album name..."), { target: { value: "Holiday" } });
    await fireEvent.click(screen.getByRole("button", { name: "Create" }));
    await fireEvent.click(within(dialog).getByRole("button", { name: "Add to album" }));

    await waitFor(() => expect(catalogue.create).toHaveBeenCalledWith("Holiday"));
    await waitFor(() => expect(addAssetToAlbum).toHaveBeenCalledWith("photo-1", "album-created"));
  });

  it("keeps the album picker open and reports a failed assignment", async () => {
    const catalogue = albumCatalogue();
    const addAssetToAlbum = vi.fn().mockRejectedValue(new Error("Network unavailable"));
    render(TimelineView, { timeline, albumCatalogue: catalogue, addAssetToAlbum });

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
    let resolveAssignment: (() => void) | undefined;
    const catalogue = albumCatalogue();
    const addAssetToAlbum = vi.fn(() => new Promise<void>((resolve) => {
      resolveAssignment = resolve;
    }));
    render(TimelineView, { timeline, albumCatalogue: catalogue, addAssetToAlbum });

    await fireEvent.click(screen.getByRole("button", { name: "Open photo" }));
    await fireEvent.click(screen.getByRole("button", { name: "Add to album" }));
    const picker = screen.getByRole("dialog", { name: "Add to album" });
    await fireEvent.click(screen.getByRole("button", { name: "Summer" }));
    await fireEvent.click(within(picker).getByRole("button", { name: "Add to album" }));

    await waitFor(() => expect(addAssetToAlbum).toHaveBeenCalledWith("photo-1", "album-summer"));
    expect(screen.getByRole("button", { name: "Close lightbox" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Previous asset" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Next asset" })).toBeDisabled();
    await fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.getByRole("dialog", { name: "Media lightbox" })).toBeInTheDocument();

    resolveAssignment?.();
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
      expect(screen.getByRole("button", { name: "Remove from favorites" })).toBeEnabled();
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
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
    fireEvent.click(screen.getByRole("button", { name: "Next asset" }));

    expect(screen.getByRole("button", { name: "Download original" })).toBeDisabled();
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
});
