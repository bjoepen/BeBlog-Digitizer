import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const eventState = vi.hoisted(() => ({ handlers: {} as Record<string, (event: { payload: unknown }) => void> }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async (name: string, handler: (event: { payload: unknown }) => void) => { eventState.handlers[name] = handler; return () => {}; }) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => "guided-test-run") }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(async () => null) }));

import App from "./App.svelte";

const device = { id: "avision:usb:002:006", displayName: "Minolta DiMAGE Scan Dual II", vendor: "Minolta", model: "DiMAGE Scan Dual II", deviceType: "film scanner" };
const batchEvent = (overrides: Record<string, unknown> = {}) => ({
  batchId: "batch-ui-test", phase: "scanning", currentFrame: 1, completedFrames: 0,
  totalFrames: 4, cooldownRemainingSeconds: 0, awaitingHolderConfirmation: false,
  scannerLeaseRetained: false, message: "Scan läuft", ...overrides,
});

describe("Filmstreifen- und Einzelbildoberfläche", () => {
  beforeEach(() => vi.clearAllMocks());
  afterEach(() => { cleanup(); eventState.handlers = {}; });

  it("startet standardmäßig im Filmstreifenmodus mit vier Bildern und 2.820 dpi", () => {
    render(App);
    expect(screen.getByRole("button", { name: "Filmstreifen" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByLabelText("Anzahl Negative")).toHaveValue("4");
    expect(screen.getByLabelText("Auflösung")).toHaveValue("2820");
    expect(screen.getAllByText("RGB8").length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: "Filmstreifen scannen" })).toBeDisabled();
    expect(screen.getAllByRole("listitem", { name: /Bild [1-4]: Wartet/ })).toHaveLength(4);
    expect(screen.queryByLabelText("Filmposition")).not.toBeInTheDocument();
  });

  it("stellt Filmstreifen mit 2–6 Positionen dar und reicht die Anzahl an den Batchaufruf durch", async () => {
    const tauri = await import("@tauri-apps/api/core");
    const dialog = await import("@tauri-apps/plugin-dialog");
    vi.mocked(tauri.invoke).mockResolvedValueOnce({ state: "oneScanner", devices: [device] });
    vi.mocked(dialog.open).mockResolvedValueOnce("/Volumes/Scan SSD/Film");
    render(App);
    const count = screen.getByLabelText("Anzahl Negative");
    expect(Array.from((count as HTMLSelectElement).options).map((option) => option.value)).toEqual(["2", "3", "4", "5", "6"]);
    await fireEvent.change(count, { target: { value: "6" } });
    expect(screen.getAllByRole("listitem", { name: /Bild [1-6]: Wartet/ })).toHaveLength(6);
    await fireEvent.click(screen.getByRole("button", { name: "Scanner erkennen / erneut suchen" }));
    await fireEvent.click(screen.getByRole("button", { name: "Scanner bestätigen" }));
    await fireEvent.click(screen.getByRole("button", { name: "Ordner auswählen …" }));
    await fireEvent.click(screen.getByRole("button", { name: "Filmstreifen scannen" }));
    await waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith("start_d6_batch", { request: {
      frames: [1, 2, 3, 4, 5, 6], dpi: 2820, device: device.id, confirmedDevice: device.id, outputDirectory: "/Volumes/Scan SSD/Film",
    } }));
  });

  it("wechselt in Einzelbildmodus und zeigt nur dessen Positionswahl und Aktionen", async () => {
    render(App);
    await fireEvent.click(screen.getByRole("button", { name: "Einzelbild" }));
    expect(screen.getByRole("button", { name: "Einzelbild" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByLabelText("Filmposition")).toBeInTheDocument();
    expect(screen.getByLabelText("Filmposition").querySelectorAll("option")).toHaveLength(6);
    expect(screen.queryByLabelText("Anzahl Negative")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Einzelbild scannen" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Hardwarefreien Ablauf simulieren" })).toBeDisabled();
  });

  it("führt Scannererkennung nur auf Klick aus und verlangt eine ausdrückliche Auswahlbestätigung", async () => {
    const tauri = await import("@tauri-apps/api/core");
    render(App);
    expect(vi.mocked(tauri.invoke)).not.toHaveBeenCalledWith("detect_minolta_scanners");
    vi.mocked(tauri.invoke).mockResolvedValueOnce({ state: "oneScanner", devices: [device] });
    await fireEvent.click(screen.getByRole("button", { name: "Scanner erkennen / erneut suchen" }));
    expect(await screen.findByText(/Ein Scanner gefunden/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Filmstreifen scannen" })).toBeDisabled();
    await fireEvent.click(screen.getByRole("button", { name: "Scanner bestätigen" }));
    expect(screen.getByText("Auswahl bestätigt: Minolta DiMAGE Scan Dual II")).toBeInTheDocument();
    expect(screen.queryByText(/Ein Scanner gefunden/)).not.toBeInTheDocument();
    expect(screen.getByText("avision:usb:002:006")).toBeInTheDocument();
  });

  it("platziert Ausgabeordner und Scanner oberhalb des Modusumschalters", async () => {
    render(App);
    const main = screen.getByRole("main");
    const headings = [...main.querySelectorAll("h3")].map((item) => item.textContent);
    expect(headings.slice(0, 2)).toEqual(["Scanner", "Speicherort"]);
    expect(screen.getByLabelText("Ausgabeordner")).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Scanmodus" })).toBeInTheDocument();
  });

  it("zeigt Filmhalteraufforderung erst nach OPENED und sendet CONTINUE nur nach Nutzerklick", async () => {
    const dialog = await import("@tauri-apps/plugin-dialog");
    const tauri = await import("@tauri-apps/api/core");
    vi.mocked(dialog.open).mockResolvedValueOnce("/tmp/be-blog-test");
    render(App);
    await fireEvent.click(screen.getByRole("button", { name: "Einzelbild" }));
    await fireEvent.click(screen.getByRole("button", { name: "Ordner auswählen …" }));
    await fireEvent.click(screen.getByRole("button", { name: "Hardwarefreien Ablauf simulieren" }));
    await waitFor(() => expect(eventState.handlers["hardware-client-event"]).toBeDefined());
    expect(screen.getByRole("button", { name: "Filmstreifen" })).toBeDisabled();
    expect(screen.queryByText(/Simulation: Halter jetzt einsetzen/)).not.toBeInTheDocument();
    eventState.handlers["hardware-client-event"]({ payload: { kind: "opened" } });
    expect(await screen.findByText(/Simulation: Halter jetzt einsetzen/)).toBeInTheDocument();
    expect(vi.mocked(tauri.invoke)).not.toHaveBeenCalledWith("confirm_guided_holder");
    await fireEvent.click(screen.getByRole("button", { name: "Filmhalter eingesetzt bestätigen" }));
    await waitFor(() => expect(vi.mocked(tauri.invoke)).toHaveBeenCalledWith("confirm_guided_holder"));
  });

  it("zeigt Simulator als nachrangige hardwarefreie Einzelbildoption und reicht keine Hardwareaktion ein", async () => {
    const dialog = await import("@tauri-apps/plugin-dialog");
    const tauri = await import("@tauri-apps/api/core");
    vi.mocked(dialog.open).mockResolvedValueOnce("/Volumes/Scan SSD/Film");
    render(App);
    await fireEvent.click(screen.getByRole("button", { name: "Einzelbild" }));
    await fireEvent.click(screen.getByRole("button", { name: "Ordner auswählen …" }));
    await fireEvent.click(screen.getByRole("button", { name: "Hardwarefreien Ablauf simulieren" }));
    await waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith("start_guided_simulation", { frame: 1, dpi: 2820, outputDirectory: "/Volumes/Scan SSD/Film" }));
    expect(vi.mocked(tauri.invoke)).not.toHaveBeenCalledWith("start_guided_scan", expect.anything());
  });

  it("bindet Bytefortschritt an echte Clientereignisse ohne Chunkzeilen im Log", async () => {
    const dialog = await import("@tauri-apps/plugin-dialog");
    vi.mocked(dialog.open).mockResolvedValueOnce("/tmp/be-blog-test");
    render(App);
    await fireEvent.click(screen.getByRole("button", { name: "Einzelbild" }));
    await fireEvent.click(screen.getByRole("button", { name: "Ordner auswählen …" }));
    await fireEvent.click(screen.getByRole("button", { name: "Hardwarefreien Ablauf simulieren" }));
    await waitFor(() => expect(eventState.handlers["hardware-client-event"]).toBeDefined());
    const send = (payload: unknown) => eventState.handlers["hardware-client-event"]({ payload });
    send({ kind: "opened" }); send({ kind: "holderConfirmationAccepted" }); send({ kind: "holderDetected" });
    send({ kind: "framePositioned", frame: 1 });
    send({ kind: "parameters", dpi: 600, width: 10, height: 10, bytesPerLine: 30, depth: 8, format: "RGB", expectedBytes: 100 });
    send({ kind: "scanStarted" });
    await screen.findByText("600 dpi · RGB8 · 10 × 10");
    const eventCountBeforeChunk = screen.getByLabelText("Ereignisprotokoll").querySelectorAll("article").length;
    send({ kind: "dataChunk", chunkBytes: 10, totalBytes: 10 });
    expect(await screen.findByText("10 / 100 Bytes")).toBeInTheDocument();
    expect(screen.getByLabelText("Fortschritt des aktuellen Frames")).toHaveAttribute("value", "10");
    expect(screen.getByText("600 dpi · RGB8 · 10 × 10")).toBeInTheDocument();
    expect(screen.getByLabelText("Ereignisprotokoll").querySelectorAll("article")).toHaveLength(eventCountBeforeChunk);
  });

  it("leitet Bildstatus ausschließlich aus Batchereignissen und completedFrames ab", async () => {
    render(App);
    const handler = eventState.handlers["d6-batch-event"];
    expect(handler).toBeDefined();
    await handler({ payload: batchEvent() });
    expect(await screen.findByLabelText("Bild 1: Wird gescannt")).toBeInTheDocument();
    expect(screen.getByLabelText("Bild 2: Wartet")).toBeInTheDocument();
    await handler({ payload: batchEvent({ phase: "interFrameCooldown", currentFrame: 1, completedFrames: 1, cooldownRemainingSeconds: 30, message: "Bereitschaftspause" }) });
    expect(await screen.findByLabelText("Bild 1: Fertig")).toBeInTheDocument();
    expect(screen.getByLabelText("Bild 2: Wartet")).toBeInTheDocument();
    expect(screen.getByText("30 s")).toBeInTheDocument();
    await handler({ payload: batchEvent({ phase: "failed", currentFrame: 2, completedFrames: 1, message: "Bild 2 konnte nicht validiert werden" }) });
    expect(await screen.findByLabelText("Bild 2: Fehlgeschlagen")).toBeInTheDocument();
    expect(screen.getByLabelText("Bild 1: Fertig")).toBeInTheDocument();
  });

  it("erhält zwei fertige Bilder bei Abbruch von Frame 3 und startet keinen Folgescan", async () => {
    const tauri = await import("@tauri-apps/api/core");
    render(App);
    const handler = eventState.handlers["d6-batch-event"];
    expect(handler).toBeDefined();

    await handler({ payload: batchEvent({
      phase: "cancelled", currentFrame: 3, completedFrames: 2, totalFrames: 4,
      message: "Batch abgebrochen; bereits archivierte Bilder bleiben erhalten",
    }) });

    expect(await screen.findByLabelText("Bild 1: Fertig")).toBeInTheDocument();
    expect(screen.getByLabelText("Bild 2: Fertig")).toBeInTheDocument();
    expect(screen.getByLabelText("Bild 3: Abgebrochen")).toBeInTheDocument();
    expect(screen.getByLabelText("Bild 4: Wartet")).toBeInTheDocument();
    expect(screen.getByText(/Filmstreifen abgebrochen · 2 von 4 Bilder fertig/)).toBeInTheDocument();
    expect(screen.queryByText(/4 von 4 Bildern digitalisiert/)).not.toBeInTheDocument();
    expect(screen.getByLabelText("Bild 1: Fertig")).toBeInTheDocument();
    expect(vi.mocked(tauri.invoke)).not.toHaveBeenCalledWith("start_d6_batch", expect.anything());
  });

  it("hält das Ereignisprotokoll ruhig während des Cooldowns und vermeidet doppelte Batch-Erfolgsmeldungen", async () => {
    render(App);
    const handler = eventState.handlers["d6-batch-event"];
    for (let seconds = 30; seconds >= 1; seconds -= 1) await handler({ payload: batchEvent({ phase: "interFrameCooldown", currentFrame: 1, completedFrames: 1, cooldownRemainingSeconds: seconds, message: `Bereitschaftspause: ${seconds}` }) });
    expect(await screen.findByText("1 s")).toBeInTheDocument();
    expect(screen.getByLabelText("Ereignisprotokoll").querySelectorAll("article")).toHaveLength(1);
    await handler({ payload: batchEvent({ phase: "completed", currentFrame: 4, completedFrames: 4, totalFrames: 4, message: "Alle Bilder exportiert" }) });
    expect(screen.getByText(/4 von 4 Bildern digitalisiert/)).toBeInTheDocument();
    expect(screen.getAllByText("Alle Bilder exportiert")).toHaveLength(1);
  });

  it("zeigt Scanner-Inspector mit Live-Bytewerten und Ereignis-Scrollfläche", () => {
    render(App);
    expect(screen.getByLabelText("Scanner-Inspector")).toBeInTheDocument();
    expect(screen.getByLabelText("Übertragungsfortschritt")).toBeInTheDocument();
    expect(screen.getByLabelText("Ereignisprotokoll")).toHaveClass("log");
  });

  it("zeigt keine dauerhafte mehrstufige Anleitung oder interne Buildbegriffe als Hauptbedienung", () => {
    render(App);
    expect(screen.queryByText(/D5|D6|Sidecar|SHA256|Clientprozess|Build-Nummer/)).not.toBeInTheDocument();
    expect(screen.queryByText(/1 · Scanner vorbereiten/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Filmhalter jetzt einsetzen/)).not.toBeInTheDocument();
  });
});
