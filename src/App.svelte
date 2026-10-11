<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { applyScanEvent, initialSnapshot } from "./lib/state";
  import type { AppSnapshot, ScanEvent, SimulationMode, StartScanRequest } from "./lib/types";

  let snapshot: AppSnapshot = initialSnapshot;
  let frame = 1;
  let simulation: SimulationMode = "success";
  let events: ScanEvent[] = [];
  let commandError = "";

  $: active = snapshot.activeJobId !== null;
  $: stateLabel = ({
    idle: "Bereit", initializing: "Initialisierung", openingScanner: "Scanner wird geöffnet",
    waitingForHolder: "Warte auf Filmhalter", configuringFrame: "Frame wird konfiguriert",
    startingScan: "Scan startet", transferring: "Datenübertragung", finalizing: "Dateien werden finalisiert",
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

  async function startScan() {
    commandError = "";
    events = [];
    const request: StartScanRequest = { frame, simulation };
    try {
      const jobId = await invoke<string>("start_scan", { request });
      snapshot = { ...snapshot, scannerStatus: "busy", activeJobId: jobId, state: "initializing", progress: 0 };
    } catch (error) {
      commandError = String(error);
    }
  }

  async function cancelScan() {
    commandError = "";
    try { await invoke("cancel_scan"); }
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

  <div class="workspace">
    <section class="card capture">
      <div class="section-heading">
        <div><p class="step">AUFNAHME</p><h2>Neuer Scan</h2></div>
        <span class="safe-note">Simulation · keine Hardware</span>
      </div>

      <div class="control-grid">
        <label>Frame
          <select bind:value={frame} disabled={active}>
            {#each [1, 2, 3, 4] as value}<option value={value}>Frame {value}</option>{/each}
          </select>
        </label>
        <div class="parameter"><span>Farbe</span><strong>RGB8</strong></div>
        <div class="parameter"><span>Auflösung</span><strong>600 dpi</strong></div>
      </div>

      <label class="scenario">Testszenario
        <select bind:value={simulation} disabled={active}>
          <option value="success">Research-009-Erfolg</option>
          <option value="error">Simulierter Lesefehler</option>
          <option value="timeout">Simulierter Halter-Timeout</option>
          <option value="incomplete">Unvollständiger Datenstrom</option>
        </select>
      </label>

      <div class="progress-head"><span>{stateLabel}</span><strong>{snapshot.progress}%</strong></div>
      <div class="progress" aria-label="Scanfortschritt" aria-valuenow={snapshot.progress} role="progressbar">
        <div style={`width: ${snapshot.progress}%`}></div>
      </div>

      {#if commandError}<p class="error-banner" role="alert">{commandError}</p>{/if}

      <div class="actions">
        <button class="primary" on:click={startScan} disabled={active}>Simulierten Scan starten</button>
        <button class="secondary" on:click={cancelScan} disabled={!active}>Abbrechen</button>
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
