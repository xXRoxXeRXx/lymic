import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import "../i18n";
import VirtualLibraryNoticeBanner from "./VirtualLibraryNoticeBanner.svelte";
import LoginView from "./LoginView.svelte";

afterEach(cleanup);

describe("VirtualLibraryNoticeBanner", () => {
  it("tells which photo could not be opened and why, and can be dismissed", async () => {
    const onDismiss = vi.fn();
    render(VirtualLibraryNoticeBanner, {
      notice: { name: "IMG_1.jpg", key: "log_virtual_library_offline", canRelogin: false },
      onRelogin: vi.fn(),
      onDismiss,
    });

    expect(screen.getByRole("alert")).toHaveTextContent("Could not open “IMG_1.jpg” from the Virtual Library because Immich could not be reached.");
    expect(screen.queryByRole("button", { name: "Sign in again" })).not.toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "Dismiss notice" }));
    expect(onDismiss).toHaveBeenCalledOnce();
  });

  it("offers to sign in again when the sign-in expired", async () => {
    const onRelogin = vi.fn();
    render(VirtualLibraryNoticeBanner, {
      notice: { name: "IMG_1.jpg", key: "log_virtual_library_unauthorized", canRelogin: true },
      onRelogin,
      onDismiss: vi.fn(),
    });

    await fireEvent.click(screen.getByRole("button", { name: "Sign in again" }));

    expect(onRelogin).toHaveBeenCalledOnce();
  });
});

describe("LoginView when signing in again", () => {
  const props = { serverUrl: "https://immich.example", apiKey: "", allowLocalHttp: false, cloudflareClientId: "", cloudflareClientSecret: "", isLoggingIn: false, loginError: "", onLogin: vi.fn() };

  it("explains why and lets the user leave without signing in", async () => {
    const onCancel = vi.fn();
    render(LoginView, { ...props, reloginNotice: "Your Immich sign-in is no longer valid.", onCancel });

    expect(screen.getByText("Your Immich sign-in is no longer valid.")).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "Not now" }));
    expect(onCancel).toHaveBeenCalledOnce();
  });

  it("is the plain login form without a notice or a way out", () => {
    render(LoginView, props);

    expect(screen.queryByRole("button", { name: "Not now" })).not.toBeInTheDocument();
  });
});
