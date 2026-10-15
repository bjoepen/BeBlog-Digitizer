<script lang="ts">
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { invoke } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import { confirmHolderInsertion, initialGuidedProjection, projectHardwareEvent, type HardwareClientEvent } from "./lib/guided";

  type D6BatchEvent = { batchId: string; phase: string; currentFrame: number | null; completedFrames: number; totalFrames: number; cooldownRemainingSeconds: number; awaitingHolderConfirmation: boolean; scannerLeaseRetained: boolean; message: string };
  type ScanParameters = Extract<HardwareClientEvent, { kind: "parameters" }>;
  type ViewMode = "strip" | "single";

  let projection = initialGuidedProjection;
  let frame = 1;
  let viewMode: ViewMode = "strip";
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
  $: stripItems = Array.from({ length: batchSize }, (_, index) => index + 1).map((position) => {
    if (!d6Batch) return { position, state: "waiting", label: "Wartet" };
    if (position <= d6Batch.completedFrames) return { position, state: "completed", label: "Fertig" };
    if (d6Batch.currentFrame === position && d6Batch.phase === "failed") return { position, state: "failed", label: "Fehlgeschlagen" };
    if (d6Batch.currentFrame === position && d6Batch.phase === "cancelled") return { position, state: "cancelled", label: "Abgebrochen" };
    if (d6Batch.currentFrame === position && d6Batch.phase === "scanning") return { position, state: "scanning", label: "Wird gescannt" };
    if (d6Batch.currentFrame === position && ["starting", "awaitingHolderConfirmation", "waitingForHolderDetection"].includes(d6Batch.phase)) return { position, state: "preparing", label: "Wird vorbereitet" };
    if (d6Batch.currentFrame === position && d6Batch.phase === "cancelling") return { position, state: "preparing", label: "Abbruch läuft" };
    return { position, state: "waiting", label: "Wartet" };
  });
  $: stripActive = viewMode === "strip" && d6BatchActive;
  const d6PhaseNames: Record<string, string> = {
    starting: "Prozess wird vorbereitet", awaitingHolderConfirmation: "Warte auf Halterbestätigung",
    waitingForHolderDetection: "Warte auf Haltererkennung", scanning: "Scan läuft",
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
    discoveryMessage = `${selected?.displayName ?? "Scanner"} bestätigt.`;
  }

  function scannerSelectionChanged() {
    confirmedDevice = "";
    if (device) discoveryMessage = "Scanner ausgewählt. Bitte die Auswahl ausdrücklich bestätigen.";
  }

  function clearFinishedBatchView() {
    if (!d6BatchActive) d6Batch = null;
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
    d6Batch = null;
    exportNotice = "";
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
    d6Batch = null;
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
    <div><p class="eyebrow">BeBlog · Filmwerkstatt</p><h1>BeBlog Digitizer</h1><p class="subtitle">Geführte Digitalisierung von Kleinbildnegativen.</p></div>
      <div class:locked={simulationMode} class="adapter-badge"><span></span>{simulationMode ? "Hardwarefreie Simulation · keine Scannerverbindung" : "Für die Filmdigitalisierung"}</div>
  </header>

  <div class="workspace">
    <section class="card capture">
      <div class="section-heading"><div><p class="step">FILMDIGITALISIERUNG</p><h2>Aufnahme vorbereiten</h2></div><span class="safe-note">RGB8 · keine automatischen Wiederholungen</span></div>

      <section class="control-section setup-section" aria-labelledby="scanner-selection-heading">
        <div class="control-section-heading"><h3 id="scanner-selection-heading">Scanner</h3><span>Erkennung nur auf ausdrücklichen Klick</span></div>
        <div class="folder-row scanner-row"><button class="secondary" disabled={!sessionReleased || cooldownRemaining > 0 || discoveryState === "searching"} on:click={detectScanners}>{discoveryState === "searching" ? "Suche läuft …" : "Scanner erkennen / erneut suchen"}</button><select aria-label="Erkannten Scanner auswählen" bind:value={device} on:change={scannerSelectionChanged} disabled={!sessionReleased || scanners.length === 0 || cooldownRemaining > 0}><option value="">Scanner auswählen</option>{#each scanners as scanner}<option value={scanner.id}>{scanner.displayName}</option>{/each}</select><button class="secondary" disabled={!device || confirmedDevice === device || !sessionReleased} on:click={confirmScannerSelection}>Scanner bestätigen</button></div>
        <p class="hint">Die Erkennung öffnet keinen Scan und bewegt den Filmhalter nicht.</p>{#if discoveryMessage}<p class="inline-status" role="status">{discoveryMessage}</p>{/if}{#if device}<details><summary>Diagnosedaten</summary><code>{device}</code></details>{/if}{#if confirmedDevice}<p class="safe-note" role="status">Auswahl bestätigt: {scanners.find((scanner) => scanner.id === confirmedDevice)?.displayName ?? confirmedDevice}</p>{/if}
      </section>

      <section class="control-section setup-section" aria-labelledby="output-folder-heading">
        <div class="control-section-heading"><h3 id="output-folder-heading">Speicherort</h3><span>Auch externe SSD möglich</span></div>
        <div class="folder-row"><input aria-label="Ausgabeordner" type="text" readonly placeholder="Noch kein Ordner ausgewählt" bind:value={outputFolder} /><button class="secondary" on:click={chooseFolder}>Ordner auswählen …</button></div>
      </section>

      <nav class="mode-switch" aria-label="Scanmodus">
        <button class:active={viewMode === "strip"} aria-pressed={viewMode === "strip"} on:click={() => viewMode = "strip"} disabled={d6BatchActive || (projection.state !== "idle" && !["completed", "failed", "cancelled", "timedOut"].includes(projection.state))}>Filmstreifen</button>
        <button class:active={viewMode === "single"} aria-pressed={viewMode === "single"} on:click={() => viewMode = "single"} disabled={d6BatchActive || (projection.state !== "idle" && !["completed", "failed", "cancelled", "timedOut"].includes(projection.state))}>Einzelbild</button>
      </nav>

      {#if viewMode === "strip"}
        <section class="mode-panel" aria-labelledby="filmstrip-heading">
          <div class="mode-title"><div><p class="step">FILMSTREIFEN</p><h2 id="filmstrip-heading">Bilder in Folge aufnehmen</h2></div><span class="safe-note">Der Halter bleibt eingesetzt</span></div>
          <div class="control-grid strip-controls">
            <label>Anzahl Negative<select aria-label="Anzahl Negative" bind:value={batchSize} on:change={clearFinishedBatchView} disabled={d6BatchActive}>{#each [2,3,4,5,6] as count}<option value={count}>{count} Bilder</option>{/each}</select></label>
            <label>Auflösung<select aria-label="Auflösung" bind:value={dpi} on:change={clearFinishedBatchView} disabled={d6BatchActive}><option value={2820}>2.820 dpi</option><option value={600}>600 dpi</option></select></label>
            <div class="parameter"><span>Farbmodus</span><strong>RGB8</strong></div>
          </div>
          <ol class="filmstrip" aria-label={`${batchSize} Bildpositionen`}>
            {#each stripItems as item (item.position)}
              <li class={`film-frame ${item.state}`} aria-label={`Bild ${item.position}: ${item.label}`}><span class="frame-number">{item.position}</span><span class="frame-window" aria-hidden="true"><i></i></span><span class="frame-label">{item.label}</span></li>
            {/each}
          </ol>
          {#if stripActive && d6Batch}
            <div class="strip-status" role="status"><strong>{d6Batch.completedFrames} von {d6Batch.totalFrames} Bildern fertig</strong><span>{d6Batch.message}</span></div>
          {/if}
          {#if d6Batch?.scannerLeaseRetained}<p class="error-banner">Cleanup unklar; der Scanner bleibt gesperrt.</p>{/if}
          {#if d6BatchActive}<div class="workflow-actions"><button class="secondary" on:click={cancelD6Batch}>Filmstreifen abbrechen</button></div>{:else}
            <button class="primary start-action" disabled={!outputFolder.trim() || !device.trim() || confirmedDevice !== device || !sessionReleased || cooldownRemaining > 0 || projection.state !== "idle"} on:click={startD6Batch}>Filmstreifen scannen</button>
          {/if}
          {#if !d6BatchActive && !d6Batch}<p class="hint contextual-hint">Den Filmhalter vor dem Start herausnehmen. Die Einlegeaufforderung erscheint nach Verbindungsaufbau.</p>{/if}
        </section>
      {:else}
        <section class="mode-panel" aria-labelledby="single-heading">
          <div class="mode-title"><div><p class="step">EINZELBILD</p><h2 id="single-heading">Ein Bild aufnehmen</h2></div></div>
          <div class="control-grid single-controls">
            <label>Filmposition<select aria-label="Filmposition" bind:value={frame} disabled={projection.state !== "idle" && projection.state !== "completed" && projection.state !== "failed" && projection.state !== "cancelled" && projection.state !== "timedOut"}>{#each [1,2,3,4,5,6] as value}<option value={value}>Position {value}</option>{/each}</select></label>
            <label>Auflösung<select aria-label="Auflösung" bind:value={dpi}><option value={2820}>2.820 dpi</option><option value={600}>600 dpi</option></select></label>
            <div class="parameter"><span>Farbmodus</span><strong>RGB8</strong></div>
          </div>
          <div class="single-actions">
            <button class="primary" disabled={d6BatchActive || !clientApproved || !outputFolder.trim() || !device.trim() || confirmedDevice !== device || projection.state !== "idle" || !sessionReleased || cooldownRemaining > 0} on:click={startGuidedScan}>Einzelbild scannen</button>
            <button class="secondary simulation-action" disabled={d6BatchActive || !outputFolder.trim() || projection.state !== "idle" || !sessionReleased} on:click={startGuidedSimulation}>Hardwarefreien Ablauf simulieren</button>
          </div>
          {#if !sessionReleased && projection.state === "openingScanner"}<p class="hint contextual-hint">Scanner wird initialisiert. Nach bestätigter Verbindung erscheint die Aufforderung zum Einsetzen.</p>{/if}
        </section>
      {/if}

      {#if cooldownRemaining > 0}<p class="safe-note cooldown-note" role="status">Nächster Scan frühestens in {cooldownRemaining} Sekunden.</p>{/if}

      {#if d6BatchActive}
        <div class="holder-guide" aria-live="polite">
          <p><strong>Bild {d6Batch?.currentFrame ?? 1} von {d6Batch?.totalFrames ?? batchSize}</strong></p>
          {#if d6Batch?.awaitingHolderConfirmation}
            <p class="prompt"><strong>Filmhalter jetzt einsetzen.</strong> Die Scannerverbindung ist geöffnet. Nach Bestätigung folgt die Haltererkennung.</p>
            <button class="primary" on:click={confirmInserted}>Filmhalter eingesetzt bestätigen</button>
          {:else if d6Batch?.phase === "interFrameCooldown"}
            <p><strong>Bereitschaftspause.</strong> Der nächste Prozess startet nach Ablauf des Rust-gesteuerten Countdowns.</p>
          {:else}
            <p>Der Filmhalter bleibt eingesetzt. Die nächste Bildposition startet nach Prozessabschluss und Bereitschaftspause.</p>
          {/if}
        </div>
      {:else}
      <div class="holder-guide contextual-guide" aria-live="polite">
        {#if projection.state === "awaitingHolderConfirmation"}
          <p class="prompt"><strong>{simulationMode ? "Simulation: Halter jetzt einsetzen." : "Filmhalter jetzt einsetzen."}</strong> {simulationMode ? "Das OPENED-Ereignis stammt vom Testprozess." : "Die Scannerverbindung ist geöffnet."}</p>
          <button class="primary" on:click={confirmInserted}>Filmhalter eingesetzt bestätigen</button>
        {:else if projection.state === "waitingForHolderDetection"}
          <p><strong>Warte auf Haltererkennung.</strong> Die Bestätigung allein bedeutet noch nicht, dass der Halter erkannt wurde.</p>
        {/if}
      </div>
      {/if}

      {#if projection.error}<p class="error-banner" role="alert">{projection.error}</p>{/if}
      {#if viewMode === "strip" && d6Batch?.phase === "completed" && d6Batch.completedFrames === d6Batch.totalFrames}<p class="success-banner" role="status">{d6Batch.completedFrames} von {d6Batch.totalFrames} Bildern digitalisiert · {outputFolder}</p>{/if}
      {#if viewMode === "strip" && d6Batch?.phase === "failed"}<p class="error-banner" role="alert">{d6Batch.completedFrames} von {d6Batch.totalFrames} Bildern abgeschlossen. Bereits gesicherte Bilder bleiben erhalten.</p>{/if}
      {#if viewMode === "strip" && d6Batch?.phase === "cancelled"}<p class="hint" role="status">Filmstreifen abgebrochen · {d6Batch.completedFrames} von {d6Batch.totalFrames} Bilder fertig.</p>{/if}
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
      {#if exportNotice && !d6Batch}<p class="success-banner" role="status">{exportNotice}</p>{/if}

      <div class="section-heading event-heading"><div><p class="step">DIAGNOSE</p><h3>Scannerereignisse</h3></div><span>{events.length}</span></div>
      <div class="log" aria-label="Ereignisprotokoll" aria-live="polite">{#if events.length === 0}<p class="empty">Noch keine Scannerereignisse. Benutzerbestätigung und Hardwareerkennung werden getrennt behandelt.</p>{:else}{#each [...events].reverse() as event}<article class:event-error={event.error}><strong>{event.label}</strong><p>{event.detail}</p></article>{/each}{/if}</div>
    </aside>
  </div>

</main>
