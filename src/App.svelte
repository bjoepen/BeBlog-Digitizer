<script lang="ts">
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import { confirmHolderInsertion, initialGuidedProjection, projectHardwareEvent, type HardwareClientEvent } from "./lib/guided";

  let projection = initialGuidedProjection;
  let frame = 1;
  let dpi = 2820;
  let outputFolder = "";
  let events: { label: string; detail: string; error?: boolean }[] = [];
  const clientApproved = false;
  const stateNames: Record<string, string> = {
    idle: "Bereit", openingScanner: "Scanner wird initialisiert", awaitingHolderConfirmation: "Verbindung bestätigt",
    waitingForHolderDetection: "Warte auf tatsächliche Haltererkennung", positioningFrame: "Frame wird positioniert",
    configuringFrame: "Scan wird vorbereitet", scanning: "Scan läuft", transferring: "Datenübertragung",
    finalizing: "Artefakte werden geprüft", closingScanner: "Scannerverbindung wird geschlossen",
    completed: "Scan abgeschlossen", failed: "Scan fehlgeschlagen"
  };

  onMount(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    void listen<HardwareClientEvent>("hardware-client-event", ({ payload }) => {
      projection = projectHardwareEvent(projection, payload);
      events = [...events.slice(-99), { label: payload.kind, detail: payload.kind === "dataChunk" ? `${payload.totalBytes.toLocaleString("de-DE")} Byte übertragen` : payload.kind === "error" || payload.kind === "openFailed" ? payload.detail : payload.kind }];
    }).then((fn) => { if (disposed) fn(); else unlisten = fn; });
    return () => { disposed = true; unlisten?.(); };
  });

  function confirmInserted() {
    projection = confirmHolderInsertion(projection);
    // The process controller is intentionally not connected until independent
    // binary approval; this updates only the typed presentation reducer.
    events = [...events, { label: "Benutzerbestätigung", detail: "Einsetzen bestätigt; dies ist noch keine Haltererkennung." }];
  }

  async function chooseFolder() {
    const selected = await open({ directory: true, multiple: false, title: "Scan-Ausgabeordner auswählen" });
    if (typeof selected === "string") outputFolder = selected;
  }
</script>

<svelte:head><title>BeBlog Digitizer</title></svelte:head>

<main>
  <header>
    <div><p class="eyebrow">BeBlog · Filmwerkstatt</p><h1>BeBlog Digitizer</h1><p class="subtitle">Geführter Einzelbild-Workflow für den Minolta DiMAGE Scan Dual II.</p></div>
    <div class="adapter-badge locked"><span></span>Hardwarestart gesperrt</div>
  </header>

  <section class="status-strip" aria-label="Scannerstatus">
    <div><span>Scanner</span><strong>{stateNames[projection.state] ?? "Nicht verbunden"}</strong></div>
    <div><span>Filmhalter</span><strong>{projection.holderDetected ? "Vom Scanner erkannt" : "Noch nicht erkannt"}</strong></div>
    <div><span>Übertragung</span><strong>{projection.expectedBytes === null ? "—" : `${projection.transferredBytes.toLocaleString("de-DE")} / ${projection.expectedBytes.toLocaleString("de-DE")} Byte`}</strong></div>
  </section>

  <div class="workspace">
    <section class="card capture">
      <div class="section-heading"><div><p class="step">EINZELBILDSCAN</p><h2>Scan vorbereiten</h2></div><span class="safe-note">RGB8 · keine automatische Wiederholung</span></div>

      <div class="control-grid">
        <label>Frame<select bind:value={frame}>{#each [1,2,3,4] as value}<option value={value}>Frame {value}</option>{/each}</select></label>
        <label>Auflösung<select bind:value={dpi}><option value={2820}>2.820 dpi · Standard</option><option value={600}>600 dpi · Referenz</option></select></label>
        <div class="parameter"><span>Farbmodus</span><strong>RGB8</strong></div>
      </div>

      <div class="folder"><span>Ausgabeordner</span><div class="folder-row"><input aria-label="Ausgabeordner" type="text" readonly placeholder="Noch kein Ordner ausgewählt" bind:value={outputFolder} /><button class="secondary" on:click={chooseFolder}>Ordner auswählen …</button></div></div>
      <p class="hint">Der Zielordner kann auf einer externen SSD liegen. Vor Freigabe wird pro Aufnahme ein neuer, kollisionsfreier Unterordner verwendet.</p>

      <div class="holder-guide" aria-live="polite">
        <p><strong>1 · Scanner vorbereiten.</strong> Filmhalter herausnehmen, Klappe schließen und Scanner einschalten.</p>
        <p><strong>2 · Verbindung aufbauen.</strong> Die App wartet auf das bestätigte <code>sane_open</code>-Ereignis.</p>
        {#if projection.state === "awaitingHolderConfirmation"}
          <p class="prompt"><strong>3 · Filmhalter jetzt einsetzen.</strong> Erst jetzt wird die Einlegeaufforderung gezeigt.</p>
          <button class="primary" on:click={confirmInserted}>Filmhalter eingesetzt bestätigen</button>
        {:else if projection.state === "waitingForHolderDetection"}
          <p><strong>4 · Warte auf Haltererkennung.</strong> Die Bestätigung allein setzt den Halterstatus nicht auf erkannt.</p>
        {:else}
          <p><strong>3 · Halteraufforderung folgt nach Verbindung.</strong> Die UI zeigt diesen Schritt erst nach einem echten Clientereignis.</p>
        {/if}
      </div>

      {#if projection.error}<p class="error-banner" role="alert">{projection.error}</p>{/if}
      <div class="gate" role="status"><strong>Realer Start derzeit gesperrt.</strong><span>{clientApproved ? "Client ist freigegeben." : "Der D4-Clienthash ist noch nicht unabhängig freigegeben. Es wird kein Prozess gestartet."}</span></div>
      <div class="actions"><button class="primary" disabled={!clientApproved || !outputFolder.trim() || projection.state !== "idle"}>Einzelbildscan starten</button><button class="secondary" disabled>Abbrechen</button></div>
    </section>

    <aside class="card log-card"><div class="section-heading"><div><p class="step">DIAGNOSE</p><h2>Scannerereignisse</h2></div><span>{events.length}</span></div>
      <div class="log" aria-live="polite">{#if events.length === 0}<p class="empty">Noch keine Scannerereignisse. Benutzerbestätigung und Hardwareerkennung werden getrennt behandelt.</p>{:else}{#each [...events].reverse() as event}<article class:event-error={event.error}><strong>{event.label}</strong><p>{event.detail}</p></article>{/each}{/if}</div>
    </aside>
  </div>

  <section class="future-grid"><article class="placeholder"><p class="step">SCANARCHIV</p><h3>Originale bewahren</h3><p>RAW, TIFF, Parameter und Clientlog werden erst nach unabhängiger Validierung übernommen.</p><span>Validierung erforderlich</span></article><article class="placeholder"><p class="step">BILDENTWICKLUNG</p><h3>Separat entwickeln</h3><p>Farbkorrektur und Derivate bleiben vom Scanprozess getrennt.</p><span>Getrennte Verarbeitung</span></article></section>
</main>
