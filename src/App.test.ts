import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(async () => null) }));

import App from "./App.svelte";

describe("App", () => {
  afterEach(() => cleanup());

  it("shows single-frame controls but keeps real scanner start locked pending hash approval", () => {
    render(App);
    expect(screen.getByRole("heading", { name: "BeBlog Digitizer" })).toBeInTheDocument();
    expect(screen.getByLabelText("Frame")).toBeInTheDocument();
    expect(screen.getByLabelText("Auflösung")).toBeInTheDocument();
    expect(screen.getByLabelText("Ausgabeordner")).toBeInTheDocument();
    expect(screen.getByText("RGB8")).toBeInTheDocument();
    expect(screen.getByText(/Der D4-Clienthash ist noch nicht unabhängig freigegeben/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Einzelbildscan starten" })).toBeDisabled();
  });

  it("does not show a holder-insertion prompt before an OPENED event", () => {
    render(App);
    expect(screen.queryByText(/Filmhalter jetzt einsetzen/)).not.toBeInTheDocument();
    expect(screen.getByText(/erst nach einem echten Clientereignis/)).toBeInTheDocument();
  });

  it("allows choosing an external archive folder without starting a scan", async () => {
    const dialog = await import("@tauri-apps/plugin-dialog");
    vi.mocked(dialog.open).mockResolvedValueOnce("/Volumes/Scan SSD/Film");
    render(App);
    await fireEvent.click(screen.getByRole("button", { name: "Ordner auswählen …" }));
    expect(screen.getByLabelText("Ausgabeordner")).toHaveValue("/Volumes/Scan SSD/Film");
    expect(screen.getByRole("button", { name: "Einzelbildscan starten" })).toBeDisabled();
  });
});
