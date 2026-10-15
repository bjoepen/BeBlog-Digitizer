<script lang="ts">
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { invoke } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import { confirmHolderInsertion, initialGuidedProjection, projectHardwareEvent, type HardwareClientEvent } from "./lib/guided";

  type D6BatchEvent = { batchId: string; phase: string; currentFrame: number | null; completedFrames: number; totalFrames: number; cooldownRemainingSeconds: number; awaitingHolderConfirmation: boolean; scannerLeaseRetained: boolean; message: string };
  type ScanParameters = Extract<HardwareClientEvent, { kind: "parameters" }>;

  let projection = initialGuidedProjection;
  let frame = 1;
  let dpi = 2820;
  let outputFolder = "";
  let device = "";
  let confirmedDevice = "";
  let scanners: { id: string; displayName: string; vendor: string; model: string; deviceType: string }[] = [];
  let discoveryState: "notChecked" | "searching" | "noScanner" | "oneScanner" | "multipleScanners" | "error" = "notChecked";
  let discoveryMessage = "";
  let cooldownRemaining = 0;
  let sessionReleased = true;
  let cooldownTimer: ReturnType<typeof setInterval> | undefined;
  let exportNotice = "";
  let simulationMode = false;
  let events: { label: string; detail: string; error?: boolean }[] = [];
  let batchSize = 4;
  let d6Batch: D6BatchEvent | null = null;
  let d6BatchActive = false;
  let latestParameters: ScanParameters | null = null;
  let lastBatchLogKey = "";
  let lastD6Frame: number | null = null;
  $: inspectorCooldown = d6BatchActive ? (d6Batch?.cooldownRemainingSeconds ?? 0) : cooldownRemaining;
  $: showLastBatchState = Boolean(d6Batch && !d6BatchActive && projection.state === "idle");
  $: inspectorProcessState = d6Batch && (d6BatchActive || showLastBatchState)
    ? (d6PhaseNames[d6Batch.phase] ?? d6Batch.phase)
    : (stateNames[projection.state] ?? "Nicht verbunden");
  $: inspectorStatus = d6Batch && showLastBatchState
    ? (d6Batch.phase === "failed" ? "Fehler" : d6Batch.phase === "cancelled" ? "Abgebrochen" : "Abgeschlossen")
    : (!sessionReleased || d6BatchActive ? "Aktiv" : "Bereit");
  const d6PhaseNames: Record<string, string> = {
    starting: "Prozess wird vorbereitet", awaitingHolderConfirmation: "Warte auf Halterbestätigung",
    waitingForHolderDetection: "Warte auf Haltererkennung", scanning: "Scan läuft",
    finalizing: "Artefakte werden geprüft", closingScanner: "Scannerverbindung wird geschlossen",
    interFrameCooldown: "Bereitschaftspause", completed: "Batch abgeschlossen",
    failed: "Batch fehlgeschlagen", cancelled: "Batch abgebrochen", cancelling: "Abbruch läuft"
  };
  const clientApproved = true;
  const stateNames: Record<string, string> = {
    idle: "Bereit", openingScanner: "Scanner wird initialisiert", awaitingHolderConfirmation: "Verbindung bestätigt",
    waitingForHolderDetection: "Warte auf tatsächliche Haltererkennung", positioningFrame: "Frame wird positioniert",
    configuringFrame: "Scan wird vorbereitet", scanning: "Scan läuft", transferring: "Datenübertragung",
    finalizing: "Artefakte werden geprüft", closingScanner: "Scannerverbindung wird geschlossen",
    completed: "Scan abgeschlossen", cancelled: "Scan abgebrochen", timedOut: "Zeitüberschreitung", failed: "Scan fehlgeschlagen"
  };

  onMount(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    void listen<HardwareClientEvent>("hardware-client-event", ({ payload }) => {
      projection = projectHardwareEvent(projection, payload);
      if (payload.kind === "parameters") latestParameters = payload;
      if (payload.kind === "scanCooldownStarted") startCooldownPolling(payload.seconds);
      if (payload.kind === "sessionReleased" && !d6BatchActive) sessionReleased = true;
      if (payload.kind !== "dataChunk") {
        appendEvent(
          payload.kind === "holderConfirmationAccepted" ? "Benutzerbestätigung" : payload.kind,
          payload.kind === "holderConfirmationAccepted" ? "Einsetzen durch Benutzer bestätigt; Haltererkennung folgt getrennt." : payload.kind === "error" || payload.kind === "openFailed" || payload.kind === "processFailed" ? payload.detail : payload.kind === "diagnostic" ? `[${payload.stream}] ${payload.message}` : payload.kind === "artifactsExported" ? payload.directory : payload.kind === "processExited" ? `OS-Exitcode ${payload.exitCode}` : payload.kind
        );
      }
      if (payload.kind === "artifactsExported") exportNotice = `Validierte Dateien exportiert: ${payload.directory}`;
    }).then((fn) => { if (disposed) fn(); else unlisten = fn; });
    let unlistenBatch: UnlistenFn | undefined;
    void listen<D6BatchEvent>("d6-batch-event", ({ payload }) => {
      if (payload.currentFrame !== null && payload.currentFrame !== lastD6Frame) {
        lastD6Frame = payload.currentFrame;
        projection = { ...projection, transferredBytes: 0, expectedBytes: null, holderDetected: false };
        latestParameters = null;
      }
      d6Batch = payload;
      const terminal = ["completed", "failed", "cancelled"].includes(payload.phase);
      d6BatchActive = !terminal || payload.scannerLeaseRetained;
      if (terminal && !payload.scannerLeaseRetained) sessionReleased = true;
      const logKey = `${payload.batchId}:${payload.phase}:${payload.currentFrame ?? "-"}`;
      if (logKey !== lastBatchLogKey) {
        lastBatchLogKey = logKey;
        appendEvent(`D6 ${payload.phase} · Frame ${payload.currentFrame ?? "—"}/${payload.totalFrames}`, payload.message, payload.phase === "failed");
      }
    }).then((fn) => { if (disposed) fn(); else unlistenBatch = fn; });
    return () => { disposed = true; unlisten?.(); unlistenBatch?.(); if (cooldownTimer) clearInterval(cooldownTimer); };
  });

  function appendEvent(label: string, detail: string, error = false) {
    events = [...events.slice(-99), { label, detail, error }];
  }

  async function refreshCooldown() {
    try {
      cooldownRemaining = Math.max(0, await invoke<number>("guided_scan_cooldown_remaining"));
    } catch {
      cooldownRemaining = Math.max(0, cooldownRemaining - 1);
    }
    if (cooldownRemaining === 0 && cooldownTimer) {
      clearInterval(cooldownTimer);
      cooldownTimer = undefined;
    }
  }

  function startCooldownPolling(seconds: number) {
    cooldownRemaining = seconds;
    if (cooldownTimer) clearInterval(cooldownTimer);
    void refreshCooldown();
    cooldownTimer = setInterval(() => void refreshCooldown(), 1000);
  }

  function confirmScannerSelection() {
    if (!device) return;
    confirmedDevice = device;
    const selected = scanners.find((scanner) => scanner.id === device);
    discoveryMessage = `${selected?.displayName ?? "Scanner"} bestätigt. Du kannst einen Einzelbildscan vorbereiten.`;
  }

  function scannerSelectionChanged() {
    confirmedDevice = "";
    if (device) discoveryMessage = "Scanner ausgewählt. Bitte die Auswahl ausdrücklich bestätigen.";
  }

  async function detectScanners() {
    discoveryState = "searching";
    discoveryMessage = "Suche nach Minolta DiMAGE Scan Dual II …";
    scanners = [];
    device = "";
    confirmedDevice = "";
    try {
      const result = await invoke<{ state: "noScanner" | "oneScanner" | "multipleScanners"; devices: typeof scanners }>("detect_minolta_scanners");
      scanners = result.devices;
      discoveryState = result.state;
      if (result.state === "noScanner") discoveryMessage = "Kein Minolta DiMAGE Scan Dual II gefunden.";
      else if (result.state === "oneScanner") {
        discoveryMessage = "Ein Scanner gefunden. Bitte auswählen und ausdrücklich bestätigen.";
        device = result.devices[0]?.id ?? "";
      } else discoveryMessage = `${result.devices.length} passende Scanner gefunden. Bitte den gewünschten Scanner auswählen und bestätigen.`;
    } catch (error) {
      discoveryState = "error";
      discoveryMessage = String(error);
    }
  }

  function prepareNextScan() {
    if (!sessionReleased) return;
    projection = initialGuidedProjection;
    exportNotice = "";
    sessionReleased = true;
  }

  async function confirmInserted() {
    if (d6BatchActive) {
      try { await invoke("confirm_d6_batch_holder"); }
      catch (error) { events = [...events, { label: "Batchbestätigung abgelehnt", detail: String(error), error: true }]; }
      return;
    }
    const confirmedProjection = confirmHolderInsertion(projection);
    if (confirmedProjection.state !== "waitingForHolderDetection") return;
    try {
      await invoke("confirm_guided_holder");
    } catch (error) {
      events = [...events, { label: "Bestätigung abgelehnt", detail: String(error), error: true }];
    }
  }

  async function startGuidedScan() {
    if (cooldownRemaining > 0 || !sessionReleased) return;
    simulationMode = false;
    sessionReleased = false;
    projection = { ...initialGuidedProjection, state: "openingScanner" };
    try {
      await invoke<string>("start_guided_scan", { request: { frame, dpi, device, confirmedDevice, outputDirectory: outputFolder } });
      events = [...events, { label: "Prozessstart", detail: "Der kontrollierte Clientprozess wurde gestartet." }];
    } catch (error) {
      sessionReleased = true;
      if (projection.state === "openingScanner") projection = { ...initialGuidedProjection, state: "failed", error: String(error) };
      events = [...events, { label: "Start gesperrt", detail: String(error), error: true }];
    }
  }

  async function startGuidedSimulation() {
    simulationMode = true;
    sessionReleased = false;
    exportNotice = "";
    projection = { ...initialGuidedProjection, state: "openingScanner" };
    try {
      await invoke<string>("start_guided_simulation", { frame, dpi, outputDirectory: outputFolder });
      events = [...events, { label: "Simulatorprozess", detail: "Harmloser BEBLOG/1-Kindprozess gestartet; keine Scanner- oder SANE-Zugriffe." }];
    } catch (error) {
      sessionReleased = true;
      projection = { ...initialGuidedProjection, state: "failed", error: String(error) };
      events = [...events, { label: "Simulation fehlgeschlagen", detail: String(error), error: true }];
    }
  }

  async function cancelGuidedScan() {
    try { await invoke("cancel_guided_scan"); }
    catch (error) { events = [...events, { label: "Abbruch nicht möglich", detail: String(error), error: true }]; }
  }

  async function startD6Batch() {
    if (d6BatchActive || !outputFolder.trim() || !device.trim() || confirmedDevice !== device || cooldownRemaining > 0 || !sessionReleased) return;
    simulationMode = false;
    d6BatchActive = true;
    sessionReleased = false;
    projection = { ...initialGuidedProjection, state: "openingScanner" };
    d6Batch = null;
    try {
      const frames = Array.from({ length: batchSize }, (_, index) => index + 1);
      await invoke<string>("start_d6_batch", { request: { frames, dpi, device, confirmedDevice, outputDirectory: outputFolder } });
    } catch (error) {
      d6BatchActive = false;
      sessionReleased = true;
      events = [...events, { label: "D6-Batchstart gesperrt", detail: String(error), error: true }];
    }
  }

  async function cancelD6Batch() {
    try { await invoke("cancel_d6_batch"); }
    catch (error) { events = [...events, { label: "Batchabbruch nicht möglich", detail: String(error), error: true }]; }
  }

  async function chooseFolder() {
    const selected = await open({ directory: true, multiple: false, title: "Scan-Ausgabeordner auswählen" });
    if (typeof selected === "string") outputFolder = selected;
  }
</script>

<svelte:head><title>BeBlog Digitizer</title></svelte:head>

<main>
  <header>
    <div><p class="eyebrow">BeBlog · Filmwerkstatt</p><h1>BeBlog Digitizer</h1><p class="subtitle">Geführte Einzelbild- und Batchscans für den Minolta DiMAGE Scan Dual II.</p></div>
    <div class="adapter-badge"><span></span>Minolta ARM64 · D6-Vierer-Batch bestätigt</div>
  </header>

  <div class="workspace">
    <section class="card capture">
      <div class="section-heading"><div><p class="step">SCANSTEUERUNG</p><h2>Scan vorbereiten</h2></div><span class="safe-note">RGB8 · keine automatische Wiederholung</span></div>

      <section class="control-section" aria-labelledby="scanner-selection-heading">
        <div class="control-section-heading"><h3 id="scanner-selection-heading">1 · Scanner auswählen</h3><span>Erkennung nur auf ausdrücklichen Klick</span></div>
        <div class="folder-row scanner-row"><button class="secondary" disabled={!sessionReleased || cooldownRemaining > 0 || discoveryState === "searching"} on:click={detectScanners}>{discoveryState === "searching" ? "Suche läuft …" : "Scanner erkennen / erneut suchen"}</button><select aria-label="Erkannten Scanner auswählen" bind:value={device} on:change={scannerSelectionChanged} disabled={!sessionReleased || scanners.length === 0 || cooldownRemaining > 0}><option value="">Scanner auswählen</option>{#each scanners as scanner}<option value={scanner.id}>{scanner.displayName}</option>{/each}</select><button class="secondary" disabled={!device || confirmedDevice === device || !sessionReleased} on:click={confirmScannerSelection}>Scanner bestätigen</button></div>
        <p class="hint">Die Erkennung öffnet keinen Scan und bewegt den Filmhalter nicht.</p>{#if discoveryMessage}<p class="inline-status" role="status">{discoveryMessage}</p>{/if}{#if device}<details><summary>Diagnosedaten</summary><code>{device}</code></details>{/if}{#if confirmedDevice}<p class="safe-note" role="status">Auswahl bestätigt: {scanners.find((scanner) => scanner.id === confirmedDevice)?.displayName ?? confirmedDevice}</p>{/if}
      </section>

      <section class="control-section" aria-labelledby="scan-profile-heading">
        <div class="control-section-heading"><h3 id="scan-profile-heading">2 · Scanparameter</h3><span>Für Einzelbild und Batch</span></div>
        <div class="control-grid">
          <label>Filmposition<select bind:value={frame}>{#each [1,2,3,4,5,6] as value}<option value={value}>Frame {value}</option>{/each}</select></label>
          <label>Auflösung<select bind:value={dpi}><option value={2820}>2.820 dpi · Standard</option><option value={600}>600 dpi · Referenz</option></select></label>
          <div class="parameter"><span>Farbmodus</span><strong>RGB8</strong></div>
        </div>
      </section>

      <section class="control-section" aria-labelledby="output-folder-heading">
        <div class="control-section-heading"><h3 id="output-folder-heading">3 · Ausgabeordner</h3><span>Auch externe SSD möglich</span></div>
        <div class="folder-row"><input aria-label="Ausgabeordner" type="text" readonly placeholder="Noch kein Ordner ausgewählt" bind:value={outputFolder} /><button class="secondary" on:click={chooseFolder}>Ordner auswählen …</button></div>
      </section>

      <section class="control-section workflow-section" aria-labelledby="workflow-heading">
        <div class="control-section-heading"><h3 id="workflow-heading">4 · Scanworkflow wählen</h3><span>Einzelscan oder geführte Folge</span></div>
        <div class="workflow-grid">
          <section class="workflow-card single-workflow" aria-labelledby="single-workflow-heading">
            <h4 id="single-workflow-heading">Einzelbildscan</h4>
            <p class="hint">Frame {frame} wird in einem eigenen Clientprozess aufgenommen.</p>
            <div class="workflow-card-actions"><button class="primary" disabled={d6BatchActive || !clientApproved || !outputFolder.trim() || !device.trim() || confirmedDevice !== device || projection.state !== "idle" || !sessionReleased || cooldownRemaining > 0} on:click={startGuidedScan}>Realen Einzelbildscan starten</button><button class="secondary" disabled={d6BatchActive || !outputFolder.trim() || projection.state !== "idle" || !sessionReleased} on:click={startGuidedSimulation}>Ablauf hardwarefrei simulieren</button></div>
          </section>
          <section class="workflow-card d6-panel" aria-labelledby="batch-workflow-heading">
            <h4 id="batch-workflow-heading">Geführter Mehrbildscan · D6</h4>
            <div class="folder-row"><label>Anzahl Bilder<select bind:value={batchSize} disabled={d6BatchActive}>{#each [2,3,4,5,6] as count}<option value={count}>{count} Bilder · Frames 1–{count}</option>{/each}</select></label><button class="primary" disabled={d6BatchActive || !outputFolder.trim() || !device.trim() || confirmedDevice !== device || !sessionReleased || cooldownRemaining > 0} on:click={startD6Batch}>Batch mit {batchSize} Bildern starten</button></div>
        <p class="hint">Standardmäßig vier Bilder. Jeder Frame erhält einen eigenen D5-Clientprozess; der Halter bleibt eingesetzt, wird pro Prozess erneut erkannt und zwischen erfolgreichen Frames gilt der 30-Sekunden-Cooldown.</p>
        {#if d6Batch}<div class="batch-progress" role="status"><strong>{d6Batch.currentFrame ? `Frame ${d6Batch.currentFrame} von ${d6Batch.totalFrames}` : `${d6Batch.totalFrames} Frames geplant`}</strong><span>{d6Batch.message}</span><progress max={d6Batch.totalFrames} value={d6Batch.completedFrames}></progress>{#if d6Batch.cooldownRemainingSeconds > 0}<span>Bereitschaftspause: {d6Batch.cooldownRemainingSeconds} Sekunden</span>{/if}{#if d6Batch.scannerLeaseRetained}<span class="error-banner">Prozess-Cleanup ungeklärt; Scanner bleibt gesperrt.</span>{/if}</div>{/if}
          </section>
        </div>
        {#if d6BatchActive}<div class="workflow-actions"><button class="secondary" on:click={cancelD6Batch}>D6-Batch abbrechen</button></div>{/if}
      </section>

      {#if cooldownRemaining > 0}<p class="safe-note" role="status">Nächster realer Scan frühestens in {cooldownRemaining} Sekunden. Der Cooldown wird im Rust-Core erzwungen.</p>{/if}

      {#if d6BatchActive}
        <div class="holder-guide" aria-live="polite">
          <p><strong>D6-Batch · Frame {d6Batch?.currentFrame ?? 1} von {d6Batch?.totalFrames ?? batchSize}</strong> Der bestehende D5-Einzelframe-Prozess wird für jeden Frame neu gestartet.</p>
          {#if d6Batch?.awaitingHolderConfirmation}
            <p class="prompt"><strong>Filmhalter jetzt einsetzen.</strong> Das echte <code>OPENED</code>-Ereignis ist eingetroffen. Nach Bestätigung wird die tatsächliche Haltererkennung abgewartet.</p>
            <button class="primary" on:click={confirmInserted}>Filmhalter eingesetzt bestätigen</button>
          {:else if d6Batch?.phase === "interFrameCooldown"}
            <p><strong>Bereitschaftspause.</strong> Der nächste Prozess startet nach Ablauf des Rust-gesteuerten Countdowns.</p>
          {:else}
            <p>Der Filmhalter bleibt eingesetzt. Folgeframes werden erst nach neuem Prozessstart, <code>OPENED</code>, automatischem <code>CONTINUE</code> und tatsächlicher <code>HOLDER_DETECTED</code>-Rückmeldung fortgesetzt.</p>
          {/if}
        </div>
      {:else}
      <div class="holder-guide" aria-live="polite">
        <p><strong>1 · {simulationMode ? "Simulator vorbereiten." : "Scanner vorbereiten."}</strong> {simulationMode ? "Der Testprozess erzeugt ausschließlich synthetische Dateien." : "Filmhalter herausnehmen, Klappe schließen und Scanner einschalten."}</p>
        <p><strong>2 · Verbindung aufbauen.</strong> Die App wartet auf das {simulationMode ? "simulierte " : "bestätigte "}<code>OPENED</code>-Ereignis.</p>
        {#if projection.state === "awaitingHolderConfirmation"}
          <p class="prompt"><strong>3 · {simulationMode ? "Simulation: Filmhalter jetzt einsetzen." : "Filmhalter jetzt einsetzen."}</strong> Erst nach dem OPENED-Ereignis wird die Einlegeaufforderung gezeigt.</p>
          <button class="primary" on:click={confirmInserted}>Filmhalter eingesetzt bestätigen</button>
        {:else if projection.state === "waitingForHolderDetection"}
          <p><strong>4 · Warte auf Haltererkennung.</strong> Die Bestätigung allein setzt den Halterstatus nicht auf erkannt.</p>
        {:else}
          <p><strong>3 · Halteraufforderung folgt nach Verbindung.</strong> Die UI zeigt diesen Schritt erst nach einem echten Clientereignis.</p>
        {/if}
      </div>
      {/if}

      {#if projection.error}<p class="error-banner" role="alert">{projection.error}</p>{/if}
      {#if exportNotice}<p class="success-banner" role="status">{exportNotice}</p>{/if}
      <div class="gate" role="status"><strong>Hashgebundener Hardwarepfad · Einzelbild- und D6-Batchbetrieb.</strong><span>Keine automatischen Wiederholungen; Folgeframes ausschließlich nach Prozesscleanup und Cooldown.</span></div>
      <div class="actions">{#if projection.state !== "idle" && !["completed", "failed", "cancelled", "timedOut"].includes(projection.state) && !d6BatchActive}<button class="secondary" on:click={cancelGuidedScan}>Einzelbildscan abbrechen</button>{/if}{#if ["completed", "failed", "cancelled", "timedOut"].includes(projection.state) && sessionReleased && !d6BatchActive}<button class="secondary" on:click={prepareNextScan}>Nächsten Einzelbildscan vorbereiten</button>{/if}</div>
    </section>

    <aside class="card inspector" aria-label="Scanner-Inspector">
      <div class="inspector-heading"><div><p class="step">LIVE-INSPECTOR</p><h2>Scannerstatus</h2></div><span class:busy={!sessionReleased || d6BatchActive || (showLastBatchState && d6Batch?.phase !== "completed")} class="status-pill">{inspectorStatus}</span></div>
      <dl class="inspector-grid">
        <div><dt>Prozesszustand</dt><dd>{inspectorProcessState}</dd></div>
        <div><dt>Scanner</dt><dd class="wrap-value">{confirmedDevice ? (scanners.find((scanner) => scanner.id === confirmedDevice)?.displayName ?? confirmedDevice) : device || "Nicht ausgewählt"}</dd></div>
        <div><dt>Filmhalter</dt><dd>{projection.holderDetected ? (simulationMode ? "Im Simulator erkannt" : "Vom Scanner erkannt") : "Noch nicht erkannt"}</dd></div>
        <div><dt>Frame</dt><dd>{d6Batch && (d6BatchActive || showLastBatchState) && d6Batch.currentFrame ? `Frame ${d6Batch.currentFrame} von ${d6Batch.totalFrames}` : `Frame ${frame} · Einzelscan`}</dd></div>
        <div><dt>{latestParameters ? "Zurückgelesenes Scanprofil" : "Gewähltes Scanprofil"}</dt><dd>{latestParameters ? `${latestParameters.dpi.toLocaleString("de-DE")} dpi · ${latestParameters.format}${latestParameters.depth} · ${latestParameters.width} × ${latestParameters.height}` : `${dpi.toLocaleString("de-DE")} dpi · RGB8`}</dd></div>
      </dl>

      <section class="transfer-panel" aria-label="Übertragungsfortschritt">
        <div class="transfer-heading"><strong>RAW-Datenübertragung</strong><span>{projection.expectedBytes === null ? "Warte auf Parameter" : `${projection.transferredBytes.toLocaleString("de-DE")} / ${projection.expectedBytes.toLocaleString("de-DE")} Bytes`}</span></div>
        {#if projection.expectedBytes !== null}
          <progress aria-label="Fortschritt des aktuellen Frames" max={projection.expectedBytes} value={projection.transferredBytes}></progress>
          <small>{Math.min(100, Math.round((projection.transferredBytes / projection.expectedBytes) * 100))}% · tatsächlicher Clientfortschritt</small>
        {:else}
          <div class="progress-placeholder">Noch keine Scanparameter empfangen</div>
        {/if}
      </section>

      {#if inspectorCooldown > 0}
        <div class="cooldown-status" role="status"><span>Bereitschaftspause</span><strong>{inspectorCooldown} s</strong></div>
      {/if}
      {#if projection.error}<p class="error-banner" role="alert">{projection.error}</p>{/if}
      {#if d6Batch?.scannerLeaseRetained}<p class="error-banner" role="alert">Cleanup ungeklärt; Scanner bleibt gesperrt.</p>{/if}
      {#if exportNotice}<p class="success-banner" role="status">{exportNotice}</p>{/if}

      <div class="section-heading event-heading"><div><p class="step">DIAGNOSE</p><h3>Scannerereignisse</h3></div><span>{events.length}</span></div>
      <div class="log" aria-label="Ereignisprotokoll" aria-live="polite">{#if events.length === 0}<p class="empty">Noch keine Scannerereignisse. Benutzerbestätigung und Hardwareerkennung werden getrennt behandelt.</p>{:else}{#each [...events].reverse() as event}<article class:event-error={event.error}><strong>{event.label}</strong><p>{event.detail}</p></article>{/each}{/if}</div>
    </aside>
  </div>

</main>
