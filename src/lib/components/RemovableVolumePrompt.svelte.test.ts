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

  it("renders a labelled non-blocking prompt with the volume and action buttons", () => {
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

    expect(screen.getByRole("region", { name: "SD Card Detected" })).toBeInTheDocument();
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

  it("dismisses with Escape from a prompt control", async () => {
    const onDismiss = vi.fn();
    render(RemovableVolumePrompt, {
      volume: sampleVolume,
      onAccept: vi.fn(),
      onDismiss,
    });

    await fireEvent.keyDown(screen.getByRole("button", { name: /Dismiss/i }), { key: "Escape" });
    expect(onDismiss).toHaveBeenCalledWith("vol-123");
  });

  it("does not dismiss when Escape is pressed outside the prompt", async () => {
    const onDismiss = vi.fn();
    render(RemovableVolumePrompt, {
      volume: sampleVolume,
      onAccept: vi.fn(),
      onDismiss,
    });

    await fireEvent.keyDown(window, { key: "Escape" });
    expect(onDismiss).not.toHaveBeenCalled();
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
