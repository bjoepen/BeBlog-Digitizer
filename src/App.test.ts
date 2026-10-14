import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const eventState = vi.hoisted(() => ({ handler: undefined as undefined | ((event: { payload: unknown }) => void) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async (_name: string, handler: (event: { payload: unknown }) => void) => { eventState.handler = handler; return () => {}; }) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => "guided-test-run") }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(async () => null) }));

import App from "./App.svelte";

describe("App", () => {
  beforeEach(() => vi.clearAllMocks());
  afterEach(() => { cleanup(); eventState.handler = undefined; });

  it("shows the explicitly hash-approved real scanner path while requiring device and output confirmation", async () => {
    render(App);
    expect(screen.getByRole("heading", { name: "BeBlog Digitizer" })).toBeInTheDocument();
    expect(screen.getByLabelText("Frame")).toBeInTheDocument();
    expect(screen.getByLabelText("Auflösung")).toBeInTheDocument();
    expect(screen.getByLabelText("Ausgabeordner")).toBeInTheDocument();
    expect(screen.getByText("RGB8")).toBeInTheDocument();
    expect(screen.getByText(/Realer Hardwarelauf ist freigeschaltet, aber noch nicht hardwaregetestet/)).toBeInTheDocument();
    const start = screen.getByRole("button", { name: "Realen Einzelbildscan starten" });
    expect(start).toBeDisabled();
    await fireEvent.input(screen.getByLabelText("Scanner-Gerätebezeichner"), { target: { value: "scanner:explicit" } });
    await fireEvent.click(screen.getByRole("button", { name: "Gerät bestätigen" }));
    expect(start).toBeDisabled();
    const dialog = await import("@tauri-apps/plugin-dialog");
    vi.mocked(dialog.open).mockResolvedValueOnce("/Volumes/Scan SSD/Film");
    await fireEvent.click(screen.getByRole("button", { name: "Ordner auswählen …" }));
    expect(start).toBeEnabled();
  });

  it("does not show a holder-insertion prompt before an OPENED event", () => {
    render(App);
    expect(screen.queryByText(/Filmhalter jetzt einsetzen/)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Filmhalter eingesetzt bestätigen" })).not.toBeInTheDocument();
  });

  it("shows the holder prompt only after the actual typed OPENED event", async () => {
    const dialog = await import("@tauri-apps/plugin-dialog");
    vi.mocked(dialog.open).mockResolvedValueOnce("/Volumes/Scan SSD/Film");
    render(App);
    await fireEvent.click(screen.getByRole("button", { name: "Ordner auswählen …" }));
    await fireEvent.click(screen.getByRole("button", { name: "Ablauf hardwarefrei simulieren" }));
    await waitFor(() => expect(eventState.handler).toBeDefined());
    eventState.handler!({ payload: { kind: "opened" } });
    expect(await screen.findByText(/Filmhalter jetzt einsetzen/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Filmhalter eingesetzt bestätigen" })).toBeInTheDocument();
  });

  it("does not send CONTINUE or log confirmation until the user presses the confirmation button", async () => {
    const dialog = await import("@tauri-apps/plugin-dialog");
    const tauri = await import("@tauri-apps/api/core");
    vi.mocked(dialog.open).mockResolvedValueOnce("/Volumes/Scan SSD/Film");
    render(App);
    await fireEvent.click(screen.getByRole("button", { name: "Ordner auswählen …" }));
    await fireEvent.click(screen.getByRole("button", { name: "Ablauf hardwarefrei simulieren" }));
    await waitFor(() => expect(eventState.handler).toBeDefined());
    eventState.handler!({ payload: { kind: "opened" } });
    await screen.findByRole("button", { name: "Filmhalter eingesetzt bestätigen" });
    expect(vi.mocked(tauri.invoke)).not.toHaveBeenCalledWith("confirm_guided_holder");
    expect(screen.queryByText("Benutzerbestätigung")).not.toBeInTheDocument();

    await fireEvent.click(screen.getByRole("button", { name: "Filmhalter eingesetzt bestätigen" }));
    await waitFor(() => expect(vi.mocked(tauri.invoke)).toHaveBeenCalledWith("confirm_guided_holder"));
  });

  it("allows choosing an external archive folder without starting a scan", async () => {
    const dialog = await import("@tauri-apps/plugin-dialog");
    vi.mocked(dialog.open).mockResolvedValueOnce("/Volumes/Scan SSD/Film");
    render(App);
    await fireEvent.click(screen.getByRole("button", { name: "Ordner auswählen …" }));
    expect(screen.getByLabelText("Ausgabeordner")).toHaveValue("/Volumes/Scan SSD/Film");
    expect(screen.getByRole("button", { name: "Realen Einzelbildscan starten" })).toBeDisabled();
  });

  it("starts only the explicitly labelled hardware-free guided simulation from the app UI", async () => {
    const dialog = await import("@tauri-apps/plugin-dialog");
    vi.mocked(dialog.open).mockResolvedValueOnce("/Volumes/Scan SSD/Film");
    const tauri = await import("@tauri-apps/api/core");
    render(App);
    await fireEvent.click(screen.getByRole("button", { name: "Ordner auswählen …" }));
    await fireEvent.click(screen.getByRole("button", { name: "Ablauf hardwarefrei simulieren" }));
    await waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith("start_guided_simulation", { frame: 1, dpi: 2820, outputDirectory: "/Volumes/Scan SSD/Film" }));
    expect(screen.getByText(/Realer Hardwarelauf ist freigeschaltet, aber noch nicht hardwaregetestet/)).toBeInTheDocument();
  });
});
