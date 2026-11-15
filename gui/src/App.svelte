<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { open, save } from "@tauri-apps/plugin-dialog";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import * as api from "./lib/api";
  import type { JobRow, LogLine, Progress, RunDone, Rule, Settings as SettingsData } from "./lib/types";
  import UploadBox from "./components/UploadBox.svelte";
  import RunStatus from "./components/RunStatus.svelte";
  import ResultsTable from "./components/ResultsTable.svelte";
  import JobDetails from "./components/JobDetails.svelte";
  import Settings from "./components/Settings.svelte";

  let view = $state<"main" | "settings">("main");

  // The CV
  let cvPath = $state<string | null>(api.prefs.get("cvPath"));
  let cvWords = $state<number | null>(null);
  let cvError = $state<string | null>(null);
  let dragging = $state(false);

  // The settings, for the summary line under the button
  let settings = $state<SettingsData | null>(null);
  let killDups = $state(api.prefs.get("killDups") === "1");

  // The search
  let running = $state(false);
  let current = $state<{ index: number; total: number; name: string } | null>(null);
  let doneCount = $state(0);
  let failedNames = $state<string[]>([]);
  let phase = $state("");
  let logs = $state<LogLine[]>([]);

  // The results
  let rows = $state<JobRow[]>([]);
  let resultLabel = $state<string | null>(null);
  let selected = $state<JobRow | null>(null);
  /** A message above the results. `path`: a file that "Show in folder" reveals. */
  let notice = $state<{ kind: "info" | "error"; text: string; path?: string } | null>(null);

  // The cover letter that is being written
  let writingFor = $state<JobRow | null>(null);

  let enabledSources = $derived(settings ? settings.sources.filter((s) => s.enabled).length : 0);
  let provider = $derived(settings ? settings.config.providers[settings.config.active] : null);
  let canRun = $derived(!!cvPath && !cvError && !running && enabledSources > 0);

  function basename(path: string) {
    return path.split(/[\\/]/).pop() ?? path;
  }

  async function refreshSettings() {
    try {
      settings = await api.loadSettings();
    } catch (e) {
      settings = null;
      notice = { kind: "error", text: `The settings cannot be read: ${api.errorText(e)}` };
    }
  }

  async function setCv(path: string) {
    cvPath = path;
    cvWords = null;
    cvError = null;
    api.prefs.set("cvPath", path);
    try {
      cvWords = (await api.cvInfo(path)).words;
    } catch (e) {
      cvError = api.errorText(e);
    }
  }

  async function chooseCv() {
    const path = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "CV (PDF or text)", extensions: ["pdf", "txt", "md"] }],
    });
    if (typeof path === "string") await setCv(path);
  }

  async function findJobs() {
    if (!cvPath) return;
    notice = null;
    logs = [];
    current = null;
    doneCount = 0;
    failedNames = [];
    phase = "Starting ...";
    running = true;
    try {
      await api.startRun(cvPath, killDups);
    } catch (e) {
      running = false;
      phase = "";
      notice = { kind: "error", text: api.errorText(e) };
    }
  }

  function onProgress(p: Progress) {
    if (p.step === "source") {
      current = { index: p.index, total: p.total, name: p.name };
      phase = `Reading ${p.name}`;
    } else if (p.step === "source_done") {
      doneCount += 1;
      if (p.error) failedNames.push(p.name);
    } else {
      phase = "Removing duplicates";
    }
  }

  function onDone(d: RunDone) {
    if (!running) return;
    running = false;
    phase = "";
    current = null;
    if (d.stopped) {
      notice = { kind: "info", text: "The search was stopped. The previous results are still shown." };
      return;
    }
    if (d.error) {
      notice = { kind: "error", text: d.error };
      return;
    }
    rows = d.rows;
    selected = null;
    const time = new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    resultLabel = `Search at ${time}`;
    const tokens = d.usage.reduce((sum, u) => sum + u.input + u.output, 0);
    const parts = [`Found ${d.rows.length} jobs.`];
    if (tokens > 0) parts.push(`${tokens.toLocaleString()} tokens used.`);
    if (d.failed > 0) parts.push(`${d.failed} source(s) failed: ${failedNames.join(", ")}. See the details.`);
    notice = { kind: d.failed > 0 ? "error" : "info", text: parts.join(" ") };
  }

  async function exportCsv() {
    const path = await save({ defaultPath: "jobs.csv", filters: [{ name: "CSV", extensions: ["csv"] }] });
    if (!path) return;
    try {
      await api.saveCsv(path, $state.snapshot(rows));
      notice = { kind: "info", text: `Saved ${rows.length} jobs to ${basename(path)}.` };
    } catch (e) {
      notice = { kind: "error", text: api.errorText(e) };
    }
  }

  async function openCsvFile() {
    const path = await open({ multiple: false, directory: false, filters: [{ name: "CSV", extensions: ["csv"] }] });
    if (typeof path !== "string") return;
    try {
      rows = await api.openCsv(path);
      selected = null;
      resultLabel = `Opened ${basename(path)}`;
      notice = null;
    } catch (e) {
      notice = { kind: "error", text: api.errorText(e) };
    }
  }

  async function ignore(row: JobRow, what: "job" | "company") {
    const rule: Rule =
      what === "company"
        ? { company: row.company }
        : row.url
          ? { url: row.url, note: row.title }
          : { company: row.company, title: row.title };
    try {
      await api.addIgnoreRule(rule);
      const company = row.company.toLowerCase();
      rows = rows.filter((r) => (what === "company" ? r.company.toLowerCase() !== company : r !== row));
      selected = null;
      notice = {
        kind: "info",
        text: what === "company" ? `"${row.company}" is now on the ignore list.` : "The job is now on the ignore list.",
      };
      await refreshSettings();
    } catch (e) {
      notice = { kind: "error", text: api.errorText(e) };
    }
  }

  async function writeApplication(row: JobRow) {
    if (!cvPath || writingFor) return;
    writingFor = row;
    notice = null;
    try {
      const path = await api.writeApplication(cvPath, $state.snapshot(row));
      const name = row.title || row.company;
      notice = path.endsWith(".pdf")
        ? { kind: "info", text: `The application for "${name}" is ready: ${path}`, path }
        : {
            kind: "error",
            text: `The LaTeX file for "${name}" is ready, but the PDF was not built (see the details): ${path}`,
            path,
          };
    } catch (e) {
      notice = { kind: "error", text: `The application was not written: ${api.errorText(e)}` };
    } finally {
      writingFor = null;
    }
  }

  function writeState(row: JobRow) {
    if (!cvPath || cvError) return "no-cv";
    if (writingFor === row) return "writing";
    return writingFor ? "busy" : "ready";
  }

  async function closeSettings() {
    view = "main";
    await refreshSettings();
  }

  onMount(() => {
    const unlisten = [
      listen<LogLine>("log", (e) => {
        logs.push(e.payload);
        if (logs.length > 1000) logs.splice(0, logs.length - 1000);
      }),
      listen<Progress>("progress", (e) => onProgress(e.payload)),
      listen<RunDone>("run-done", (e) => onDone(e.payload)),
      getCurrentWebview().onDragDropEvent((e) => {
        const p = e.payload;
        if (view !== "main") return;
        if (p.type === "enter" || p.type === "over") dragging = true;
        else if (p.type === "leave") dragging = false;
        else if (p.type === "drop") {
          dragging = false;
          if (p.paths.length > 0) setCv(p.paths[0]);
        }
      }),
    ];
    refreshSettings();
    if (cvPath) setCv(cvPath);
    return () => unlisten.forEach((p) => p.then((stop) => stop()));
  });
</script>

<div class="app">
  <header>
    <div class="brand">
      <svg viewBox="0 0 32 32" width="26" height="26" aria-hidden="true">
        <circle cx="16" cy="16" r="14" fill="none" stroke="currentColor" stroke-width="2" opacity="0.35" />
        <circle cx="16" cy="16" r="8.5" fill="none" stroke="currentColor" stroke-width="2" opacity="0.6" />
        <path d="M16 16 L16 2 A14 14 0 0 1 28.1 9 Z" fill="currentColor" opacity="0.85" />
        <circle cx="16" cy="16" r="2.5" fill="currentColor" />
      </svg>
      <h1>Job Radar</h1>
    </div>
    {#if view === "main"}
      <div class="header-actions">
        <button class="btn btn-ghost" onclick={openCsvFile} disabled={running}>Open CSV</button>
        <button class="btn btn-ghost" onclick={exportCsv} disabled={rows.length === 0}>Export CSV</button>
        <button class="btn" onclick={() => (view = "settings")} disabled={running}>
          <svg viewBox="0 0 20 20" width="16" height="16" aria-hidden="true">
            <path
              fill="currentColor"
              d="M11.3 1.5l.4 2.2c.5.2 1 .5 1.4.8l2.1-.8 1.3 2.3-1.7 1.4c.1.5.1 1.1 0 1.6l1.7 1.4-1.3 2.3-2.1-.8c-.4.3-.9.6-1.4.8l-.4 2.2H8.7l-.4-2.2c-.5-.2-1-.5-1.4-.8l-2.1.8-1.3-2.3 1.7-1.4a5 5 0 010-1.6L3.5 6.6l1.3-2.3 2.1.8c.4-.3.9-.6 1.4-.8l.4-2.2h2.6zM10 7a2.9 2.9 0 100 5.8A2.9 2.9 0 0010 7z"
              transform="translate(0 0.6)"
            />
          </svg>
          Settings
        </button>
      </div>
    {/if}
  </header>

  {#if view === "settings"}
    <Settings bind:killDups onClose={closeSettings} />
  {:else}
    <main>
      <section class="card search">
        <UploadBox path={cvPath} words={cvWords} error={cvError} {dragging} disabled={running} onChoose={chooseCv} />
        <div class="run">
          {#if running}
            <button class="btn btn-big" onclick={() => api.stopRun()}>Stop</button>
          {:else}
            <button class="btn btn-primary btn-big" onclick={findJobs} disabled={!canRun}>Find jobs</button>
          {/if}
          {#if settings && provider}
            <p class="muted small">
              {enabledSources} source{enabledSources === 1 ? "" : "s"} · {settings.config.active} ({provider.model})
            </p>
            {#if enabledSources === 0}
              <p class="small error-text">No active sources. Add them in the settings.</p>
            {/if}
          {/if}
        </div>
      </section>

      {#if running || logs.length > 0}
        <RunStatus {running} {phase} {current} {doneCount} {logs} />
      {/if}

      {#if notice}
        <div class="notice" class:error={notice.kind === "error"}>
          <span class="grow">{notice.text}</span>
          {#if notice.path}
            {@const path = notice.path}
            <button class="btn" onclick={() => revealItemInDir(path)}>Show in folder</button>
          {/if}
          <button class="btn btn-ghost btn-icon" aria-label="Close" onclick={() => (notice = null)}>✕</button>
        </div>
      {/if}

      <section class="results" class:with-details={!!selected}>
        <ResultsTable {rows} label={resultLabel} bind:selected />
        {#if selected}
          <JobDetails
            row={selected}
            writeState={writeState(selected)}
            onClose={() => (selected = null)}
            onWriteApplication={() => selected && writeApplication(selected)}
            onIgnoreJob={() => selected && ignore(selected, "job")}
            onIgnoreCompany={() => selected && ignore(selected, "company")}
          />
        {/if}
      </section>
    </main>
  {/if}
</div>

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 16px;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 9px;
    color: var(--accent);
  }

  .brand h1 {
    font-size: 17px;
    color: var(--text);
  }

  .header-actions {
    display: flex;
    gap: 6px;
  }

  main {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 16px;
  }

  .search {
    display: flex;
    align-items: center;
    gap: 20px;
    padding: 14px;
  }

  .run {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 6px;
    min-width: 220px;
  }

  .results {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 12px;
  }

  .results.with-details {
    grid-template-columns: minmax(0, 1fr) minmax(300px, 380px);
  }

  @media (max-width: 900px) {
    .search {
      flex-direction: column;
      align-items: stretch;
    }

    .results.with-details {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: minmax(0, 1fr) minmax(0, 1fr);
    }
  }
</style>
