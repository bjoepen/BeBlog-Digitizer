import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string) => command === "get_snapshot" ? {
    adapter: "fake", scannerStatus: "ready", holderStatus: "unknown",
    activeJobId: null, state: "idle", progress: 0, batch: null, recoveryNotices: []
  } : "job-test")
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

import App from "./App.svelte";

describe("App", () => {
  afterEach(() => cleanup());
  it("renders the safe fake scanner controls", () => {
    render(App);
    expect(screen.getByRole("heading", { name: "BeBlog Digitizer" })).toBeInTheDocument();
    expect(screen.getByText("Fake-Adapter")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Vierer-Batch vorbereiten" })).toBeEnabled();
    expect(screen.getByText(/Filmhalter mit vier Negativen vorbereiten/)).toBeInTheDocument();
    expect(screen.getByText("RGB8")).toBeInTheDocument();
    expect(screen.getByText("600 dpi")).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Unvollständiger Datenstrom" })).toBeInTheDocument();
  });

  it("requires a separate holder confirmation after batch preparation", async () => {
    render(App);
    await fireEvent.click(screen.getByRole("button", { name: "Vierer-Batch vorbereiten" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Filmhalter eingesetzt – Batch freigeben" })).toBeInTheDocument());
    expect(invoke).toHaveBeenCalledWith("start_batch", expect.objectContaining({ request: expect.objectContaining({ frames: [1, 2, 3, 4] }) }));
    expect(invoke).not.toHaveBeenCalledWith("confirm_batch_holder", expect.anything());
  });
});
