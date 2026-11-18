import { describe, expect, it, vi } from "vitest";
import type { AlbumCatalogueItem, RecognizedPerson } from "../contracts";
import { createNavigationController } from "./navigation-controller.svelte";

function setup() {
  const refresh = {
    photos: vi.fn(),
    filteredTimeline: vi.fn(),
    map: vi.fn(),
    explore: vi.fn(),
    exploreIfStale: vi.fn(),
    people: vi.fn(),
    albums: vi.fn(),
  };
  const adapter = {
    getAlbumTimeline: vi.fn().mockResolvedValue({ buckets: [], assetsByBucket: {} }),
    getPersonTimelineBuckets: vi.fn().mockResolvedValue([]),
    getPersonTimelineBucketAssets: vi.fn().mockResolvedValue([]),
  };
  let scrollTop = 0;
  const restoreScroll = vi.fn();
  const navigation = createNavigationController({
    adapter,
    refresh,
    scroll: { read: () => scrollTop, restore: restoreScroll },
  });
  return { navigation, refresh, adapter, restoreScroll, scrollTo: (top: number) => { scrollTop = top; } };
}

const album = { id: "album-1", albumName: "Holiday" } as AlbumCatalogueItem;
const person = { id: "person-1", name: "Ada" } as RecognizedPerson;

describe("navigation controller", () => {
  it("starts on the photo timeline", () => {
    expect(setup().navigation.view).toBe("photos");
  });

  describe.each([
    ["people", "openPeople", "people"],
    ["albums", "openAlbums", "albums"],
  ] as const)("opening %s", (view, open, refreshes) => {
    it("shows the view and refreshes its data once", () => {
      const { navigation, refresh } = setup();
      navigation[open]();
      expect(navigation.view).toBe(view);
      expect(refresh[refreshes]).toHaveBeenCalledTimes(1);
      navigation[open]();
      expect(refresh[refreshes]).toHaveBeenCalledTimes(1);
    });
  });

  it("opens the map, creating it once and refreshing it only when it was not already open", () => {
    const { navigation, refresh } = setup();
    expect(navigation.mapCreated).toBe(false);
    navigation.openMap();
    navigation.openMap();
    expect(navigation.view).toBe("map");
    expect(navigation.mapCreated).toBe(true);
    expect(refresh.map).toHaveBeenCalledTimes(1);
  });

  it("refreshes the Filtered Timeline that opens and not one that is already open", () => {
    const { navigation, refresh } = setup();
    navigation.openFilteredTimeline("favorites");
    navigation.openFilteredTimeline("favorites");
    expect(navigation.view).toBe("favorites");
    expect(refresh.filteredTimeline).toHaveBeenCalledTimes(1);
    expect(refresh.filteredTimeline).toHaveBeenLastCalledWith("favorites");
    navigation.openFilteredTimeline("archive");
    expect(refresh.filteredTimeline).toHaveBeenLastCalledWith("archive");
  });

  it("refreshes the Explore Overview every time it opens", () => {
    const { navigation, refresh } = setup();
    navigation.openExplore();
    navigation.openExplore();
    expect(refresh.explore).toHaveBeenCalledTimes(2);
  });

  it("keeps the Photo Timeline cache unless it was marked stale", () => {
    const { navigation, refresh } = setup();
    navigation.openPeople();
    navigation.openPhotos();
    expect(refresh.photos).not.toHaveBeenCalled();
    navigation.openPeople();
    navigation.markPhotosStale();
    navigation.openPhotos();
    navigation.openPeople();
    navigation.openPhotos();
    expect(refresh.photos).toHaveBeenCalledTimes(1);
  });

  describe("Album drill-down", () => {
    it("owns the selected Album and its timeline, which loads when selected", () => {
      const { navigation, adapter } = setup();
      navigation.openAlbums();
      navigation.selectAlbum(album);
      expect(navigation.view).toBe("album-timeline");
      expect(navigation.selectedAlbum).toEqual(album);
      expect(navigation.albumTimeline).not.toBeNull();
      expect(adapter.getAlbumTimeline).toHaveBeenCalledWith("album-1");
    });

    it("returns to the Albums list at its remembered scroll position and drops the timeline", () => {
      const { navigation, restoreScroll, scrollTo } = setup();
      navigation.openAlbums();
      scrollTo(340);
      navigation.selectAlbum(album);
      scrollTo(0);
      const timeline = navigation.albumTimeline!;
      const clear = vi.spyOn(timeline, "clear");
      navigation.returnToAlbums();
      expect(navigation.view).toBe("albums");
      expect(navigation.albumTimeline).toBeNull();
      expect(navigation.selectedAlbum).toBeNull();
      expect(clear).toHaveBeenCalled();
      expect(restoreScroll).toHaveBeenCalledWith(340);
    });
  });

  describe("Person drill-down", () => {
    it("opened from People, returns to People at its scroll position and drops the timeline", () => {
      const { navigation, refresh, adapter, restoreScroll, scrollTo } = setup();
      navigation.openPeople();
      scrollTo(120);
      navigation.selectPerson(person);
      expect(navigation.view).toBe("person-timeline");
      expect(navigation.selectedPerson).toEqual(person);
      expect(navigation.personOrigin).toBe("people");
      expect(adapter.getPersonTimelineBuckets).toHaveBeenCalledWith("person-1");
      scrollTo(0);
      navigation.returnFromPersonTimeline();
      expect(navigation.view).toBe("people");
      expect(navigation.personTimeline).toBeNull();
      expect(navigation.selectedPerson).toBeNull();
      expect(restoreScroll).toHaveBeenCalledWith(120);
      expect(refresh.exploreIfStale).not.toHaveBeenCalled();
    });

    it("opened from Explore, returns to Explore at its scroll position and refreshes it if stale", () => {
      const { navigation, refresh, restoreScroll, scrollTo } = setup();
      navigation.openExplore();
      scrollTo(75);
      navigation.selectPerson(person);
      expect(navigation.personOrigin).toBe("explore");
      scrollTo(0);
      navigation.returnFromPersonTimeline();
      expect(navigation.view).toBe("explore");
      expect(refresh.exploreIfStale).toHaveBeenCalledTimes(1);
      expect(restoreScroll).toHaveBeenCalledWith(75);
    });
  });

  it("resets to the photo timeline on logout, forgetting selections, scroll positions and the map", () => {
    const { navigation, restoreScroll, scrollTo } = setup();
    navigation.openMap();
    navigation.openPeople();
    scrollTo(90);
    navigation.selectPerson(person);
    navigation.reset();
    expect(navigation.view).toBe("photos");
    expect(navigation.mapCreated).toBe(false);
    expect(navigation.selectedPerson).toBeNull();
    expect(navigation.personTimeline).toBeNull();
    navigation.openAlbums();
    navigation.selectAlbum(album);
    navigation.reset();
    expect(navigation.selectedAlbum).toBeNull();
    expect(navigation.albumTimeline).toBeNull();
    scrollTo(0);
    navigation.selectPerson(person);
    navigation.returnFromPersonTimeline();
    expect(restoreScroll).toHaveBeenLastCalledWith(0);
  });
});
