import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import "../i18n";
import PeopleView from "./PeopleView.svelte";
import type { PeopleViewState } from "../controllers/people-controller.svelte";

function peopleState(thumbnailPath: string | null, onSelect = vi.fn()): PeopleViewState {
  return {
    people: [{ id: "person-1", name: "Ada", thumbnailPath }],
    isLoading: false,
    isRefreshing: false,
    isOffline: false,
    error: null,
    refresh: vi.fn(),
    clear: vi.fn(),
    onSelect,
  };
}

afterEach(cleanup);

describe("PeopleView thumbnails", () => {
  it("renders an available person thumbnail through the credential-safe protocol", () => {
    render(PeopleView, { people: peopleState("/profile/person-1.jpeg") });

    expect(screen.getByRole("img", { name: "Ada" })).toHaveAttribute(
      "src",
      "lymic-thumb://person/person-1",
    );
  });

  it("falls back to the neutral placeholder when the thumbnail is absent or fails", async () => {
    const { rerender } = render(PeopleView, { people: peopleState(null) });
    expect(screen.queryByRole("img", { name: "Ada" })).not.toBeInTheDocument();

    await rerender({ people: peopleState("/profile/person-1.jpeg") });
    await fireEvent.error(screen.getByRole("img", { name: "Ada" }));
    expect(screen.queryByRole("img", { name: "Ada" })).not.toBeInTheDocument();
  });
});

it("opens the selected Recognized Person", async () => {
  const onSelect = vi.fn();
  render(PeopleView, { people: peopleState(null, onSelect) });
  await fireEvent.click(screen.getByRole("button", { name: "Ada" }));
  expect(onSelect).toHaveBeenCalledWith(expect.objectContaining({ id: "person-1" }));
});
