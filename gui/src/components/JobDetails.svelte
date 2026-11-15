<script lang="ts">
  // The details of the selected job, with the link and the ignore actions.
  import { openUrl } from "@tauri-apps/plugin-opener";
  import type { JobRow } from "../lib/types";

  interface Props {
    row: JobRow;
    onClose: () => void;
    onIgnoreJob: () => void;
    onIgnoreCompany: () => void;
  }
  let { row, onClose, onIgnoreJob, onIgnoreCompany }: Props = $props();

  let link = $derived(row.url || row.source);
  let scoreClass = $derived(
    row.match_score === null ? "" : row.match_score >= 75 ? "good" : row.match_score >= 50 ? "mid" : "",
  );
</script>

<aside class="card details">
  <div class="head">
    <span class="score {scoreClass}">{row.match_score ?? "–"}</span>
    <button class="btn btn-ghost btn-icon" aria-label="Close the details" onclick={onClose}>✕</button>
  </div>
  <h2>{row.title}</h2>
  <p class="muted">{[row.company, row.location].filter(Boolean).join(" · ")}</p>

  {#if row.match_reason}
    <div class="block">
      <h3>Why it fits</h3>
      <p>{row.match_reason}</p>
    </div>
  {/if}
  {#if row.description}
    <div class="block">
      <h3>About the job</h3>
      <p>{row.description}</p>
    </div>
  {/if}

  <div class="actions">
    {#if link}
      <button class="btn btn-primary" onclick={() => openUrl(link)}>
        {row.url ? "Open the job page" : "Open the source page"}
      </button>
    {/if}
    <div class="row">
      <button class="btn" onclick={onIgnoreJob}>Ignore this job</button>
      <button class="btn" onclick={onIgnoreCompany} disabled={!row.company}>Ignore company</button>
    </div>
    <p class="hint">Ignored jobs are added to the ignore list and left out of the next searches.</p>
  </div>
</aside>

<style>
  .details {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-height: 0;
    padding: 14px 16px;
    overflow: auto;
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  h2 {
    font-size: 17px;
  }

  .block {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding-top: 10px;
    border-top: 1px solid var(--border);
  }

  .block h3 {
    color: var(--muted);
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .actions {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: auto;
    padding-top: 12px;
    border-top: 1px solid var(--border);
  }

  .row {
    display: flex;
    gap: 8px;
  }

  .row .btn {
    flex: 1;
  }
</style>
