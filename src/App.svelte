<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { applyScanEvent, initialSnapshot } from "./lib/state";
  import type { AppSnapshot, ScanEvent, SimulationMode, StartBatchRequest } from "./lib/types";

  let snapshot: AppSnapshot = initialSnapshot;
  let simulation: SimulationMode = "success";
  let failureFrame = 2;
  let events: ScanEvent[] = [];
  let commandError = "";

  $: active = snapshot.activeJobId !== null || snapshot.batch !== null;
  $: stateLabel = ({
    idle: "Bereit", initializing: "Initialisierung", openingScanner: "Scanner wird geöffnet",
    waitingForHolder: "Warte auf Filmhalter", waitingForHolderDetection: "Haltererkennung",
    awaitingHolderConfirmation: "Einlegen bestätigen", positioningFrame: "Frame wird positioniert", configuringFrame: "Frame wird konfiguriert",
    startingScan: "Scan startet", scanning: "Scan läuft", transferring: "Datenübertragung", finalizing: "Dateien werden finalisiert", closingScanner: "Verbindung wird geschlossen", interFrameCooldown: "Bereitschaftspause",
    batchCompleted: "Batch abgeschlossen", batchFailed: "Batch fehlgeschlagen", batchCancelled: "Batch abgebrochen",
    completed: "Abgeschlossen", cancelling: "Abbruch läuft", cancelled: "Abgebrochen",
    failed: "Fehler", timedOut: "Zeitüberschreitung"
  } as const)[snapshot.state];

  onMount(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    void invoke<AppSnapshot>("get_snapshot").then((value) => {
      if (!disposed) snapshot = value;
    }).catch((error) => { commandError = String(error); });
    void listen<ScanEvent>("scan-event", ({ payload }) => {
      snapshot = applyScanEvent(snapshot, payload);
      events = [...events.slice(-99), payload];
    }).then((fn) => {
      if (disposed) fn(); else unlisten = fn;
    });
    return () => { disposed = true; unlisten?.(); };
  });

  async function startBatch() {
    commandError = "";
    events = [];
    const request: StartBatchRequest = { frames: [1, 2, 3, 4], simulation, failureFrame: simulation === "success" ? null : failureFrame };
    try {
      const batchId = await invoke<string>("start_batch", { request });
      snapshot = { ...snapshot, activeJobId: batchId, state: "awaitingHolderConfirmation", progress: 0, batch: { batchId, frames: [1,2,3,4], currentFrame: null, completedFrames: 0, totalFrames: 4, cooldownRemainingSeconds: 0, awaitingHolderConfirmation: true } };
    } catch (error) {
      commandError = String(error);
    }
  }

  async function confirmHolder() { commandError = ""; try { await invoke("confirm_batch_holder"); } catch (error) { commandError = String(error); } }

  async function cancelScan() {
    commandError = "";
    try { await invoke(snapshot.batch ? "cancel_batch" : "cancel_scan"); }
    catch (error) { commandError = String(error); }
  }

  function time(ms: number) {
    return new Intl.DateTimeFormat("de-DE", { hour: "2-digit", minute: "2-digit", second: "2-digit" }).format(ms);
  }
</script>

<svelte:head><title>BeBlog Digitizer</title></svelte:head>

<main>
  <header>
    <div>
      <p class="eyebrow">BeBlog · Filmwerkstatt</p>
      <h1>BeBlog Digitizer</h1>
      <p class="subtitle">Ruhiger Arbeitsraum für reproduzierbare Filmscans.</p>
    </div>
    <div class="adapter-badge"><span></span> Fake-Adapter</div>
  </header>

  <section class="status-strip" aria-label="Gerätestatus">
    <div><span>Scanner</span><strong>{snapshot.scannerStatus === "ready" ? "Bereit" : "Beschäftigt"}</strong></div>
    <div><span>Filmhalter</span><strong>{snapshot.holderStatus === "detected" ? "35 mm erkannt" : "Noch nicht erkannt"}</strong></div>
    <div><span>Arbeitsstatus</span><strong>{stateLabel}</strong></div>
  </section>

  {#if snapshot.recoveryNotices.length > 0}<p class="error-banner" role="status">Unterbrochene Archivdaten erkannt (keine automatische Wiederaufnahme): {snapshot.recoveryNotices.join(", ")}</p>{/if}

  <div class="workspace">
    <section class="card capture">
      <div class="section-heading">
        <div><p class="step">AUFNAHME</p><h2>Neuer Scan</h2></div>
        <span class="safe-note">Simulation · keine Hardware</span>
      </div>

      <div class="control-grid">
        <label>Frame
          <select disabled><option>Frames 1–4 · Reihenfolge</option></select>
        </label>
        <div class="parameter"><span>Farbe</span><strong>RGB8</strong></div>
        <div class="parameter"><span>Auflösung</span><strong>600 dpi</strong></div>
      </div>

      <label class="scenario">Simulationsszenario
        <select bind:value={simulation} disabled={active}>
          <option value="success">Erfolgreicher Vierer-Batch</option>
          <option value="error">Prozessfehler</option><option value="timeout">Timeout</option>
          <option value="incomplete">Unvollständiger Datenstrom</option><option value="missingOutput">Fehlende Ausgabe</option><option value="invalidOutput">Ungültige TIFF-Ausgabe</option>
        </select>
      </label>
      {#if simulation !== "success"}<label class="scenario">Fehler in Frame <select bind:value={failureFrame} disabled={active}>{#each [2,3,4] as value}<option value={value}>{value}</option>{/each}</select></label>{/if}

      <div class="holder-guide" aria-live="polite">
        {#if !active}<p><strong>1 · Filmhalter mit vier Negativen vorbereiten.</strong> Der Scanner bleibt bis zum Start inaktiv.</p>
        {:else if snapshot.state === "awaitingHolderConfirmation"}<p><strong>2 · Filmhalter jetzt einsetzen.</strong> Der Halter bleibt während des gesamten Batches eingesetzt.</p><button class="primary" on:click={confirmHolder}>Filmhalter eingesetzt – Batch freigeben</button>
        {:else if snapshot.batch}<p><strong>Frame {snapshot.batch.currentFrame ?? 1} von {snapshot.batch.totalFrames}</strong> · {snapshot.batch.completedFrames} abgeschlossen</p>{#if snapshot.state === "interFrameCooldown"}<p class="countdown">Nächster Frame in {snapshot.batch.cooldownRemainingSeconds} Sekunden</p>{/if}{/if}
      </div>

      <div class="progress-head"><span>{stateLabel}</span><strong>{snapshot.progress}%</strong></div>
      <div class="progress" aria-label="Scanfortschritt" aria-valuenow={snapshot.progress} role="progressbar">
        <div style={`width: ${snapshot.progress}%`}></div>
      </div>

      {#if commandError}<p class="error-banner" role="alert">{commandError}</p>{/if}

      <div class="actions">
        <button class="primary" on:click={startBatch} disabled={active}>Vierer-Batch vorbereiten</button>
        <button class="secondary" on:click={cancelScan} disabled={!active}>Batch abbrechen</button>
      </div>
    </section>

    <aside class="card log-card">
      <div class="section-heading"><div><p class="step">PROTOKOLL</p><h2>Ereignisse</h2></div><span>{events.length}</span></div>
      <div class="log" aria-live="polite">
        {#if events.length === 0}
          <p class="empty">Noch keine Ereignisse. Der Fake-Adapter ist bereit.</p>
        {:else}
          {#each [...events].reverse() as event}
            <article class:event-error={event.level === "error"}>
              <time>{time(event.timestampMs)}</time><div><strong>{event.state}</strong><p>{event.message}</p></div>
            </article>
          {/each}
        {/if}
      </div>
    </aside>
  </div>

  <section class="future-grid">
    <article class="placeholder"><p class="step">SCANARCHIV</p><h3>Aufnahmen bewahren</h3><p>Kollisionsfreie Capture-Verzeichnisse, Manifeste und SHA-256-Prüfsummen sind im Application Core aktiv.</p><span>Grundlage aktiv</span></article>
    <article class="placeholder"><p class="step">BILDENTWICKLUNG</p><h3>Getrennt entwickeln</h3><p>Farbentwicklung und Derivate bleiben strikt vom Scannerprozess getrennt.</p><span>Vorbereitet</span></article>
  </section>
</main>
