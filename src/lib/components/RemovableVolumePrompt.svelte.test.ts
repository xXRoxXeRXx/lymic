import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import "../i18n";
import RemovableVolumePrompt from "./RemovableVolumePrompt.svelte";

afterEach(cleanup);

describe("RemovableVolumePrompt", () => {
  const sampleVolume = {
    volumeId: "vol-123",
    mountPath: "E:\\",
    displayName: "EOS_DIGITAL",
  };

  it("renders card name, mount path, and action buttons without stealing focus", () => {
    const onAccept = vi.fn();
    const onDismiss = vi.fn();

    render(RemovableVolumePrompt, {
      volume: sampleVolume,
      onAccept,
      onDismiss,
    });

    expect(screen.getByText(/EOS_DIGITAL/)).toBeInTheDocument();
    expect(screen.getByText(/E:\\/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Sync|Synchronize/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Dismiss/i })).toBeInTheDocument();

    // Verify focus is not stolen onto the buttons
    expect(document.activeElement).toBe(document.body);
  });

  it("calls onDismiss when dismiss button is clicked", async () => {
    const onAccept = vi.fn();
    const onDismiss = vi.fn();

    render(RemovableVolumePrompt, {
      volume: sampleVolume,
      onAccept,
      onDismiss,
    });

    await fireEvent.click(screen.getByRole("button", { name: /Dismiss/i }));
    expect(onDismiss).toHaveBeenCalledWith("vol-123");
  });

  it("calls onAccept when sync button is clicked", async () => {
    const onAccept = vi.fn();
    const onDismiss = vi.fn();

    render(RemovableVolumePrompt, {
      volume: sampleVolume,
      onAccept,
      onDismiss,
    });

    await fireEvent.click(screen.getByRole("button", { name: /Sync|Synchronize/i }));
    expect(onAccept).toHaveBeenCalledWith(sampleVolume);
  });
});
