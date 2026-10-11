import { render, screen } from "@testing-library/svelte";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string) => command === "get_snapshot" ? {
    adapter: "fake", scannerStatus: "ready", holderStatus: "unknown",
    activeJobId: null, state: "idle", progress: 0
  } : "job-test")
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

import App from "./App.svelte";

describe("App", () => {
  it("renders the safe fake scanner controls", () => {
    render(App);
    expect(screen.getByRole("heading", { name: "BeBlog Digitizer" })).toBeInTheDocument();
    expect(screen.getByText("Fake-Adapter")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Simulierten Scan starten" })).toBeEnabled();
    expect(screen.getByText("RGB8")).toBeInTheDocument();
    expect(screen.getByText("600 dpi")).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Unvollständiger Datenstrom" })).toBeInTheDocument();
  });
});
