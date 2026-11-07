import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import "../i18n";
import TimelineView from "./TimelineView.svelte";
import type { TimelineViewState } from "../controllers/timeline-controller.svelte";

const timeline: TimelineViewState = {
  buckets: [
    { timeBucket: "2025-03-01T00:00:00.000Z", count: 12 },
    { timeBucket: "2025-02-01T00:00:00.000Z", count: 3 },
    { timeBucket: "2024-12-01T00:00:00.000Z", count: 1 },
  ],
  isLoading: false,
  refresh: vi.fn(),
  clear: vi.fn(),
};

afterEach(cleanup);

describe("TimelineView scrubber", () => {
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
