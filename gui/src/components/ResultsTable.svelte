<script lang="ts">
  // The result table: sortable columns, a text filter and a selected row.
  import type { JobRow } from "../lib/types";

  interface Props {
    rows: JobRow[];
    label: string | null;
    selected: JobRow | null;
  }
  let { rows, label, selected = $bindable() }: Props = $props();

  type Key = "match_score" | "title" | "company" | "location" | "source";
  const columns: { key: Key; title: string }[] = [
    { key: "match_score", title: "Score" },
    { key: "title", title: "Title" },
    { key: "company", title: "Company" },
    { key: "location", title: "Location" },
    { key: "source", title: "Source" },
  ];

  let sortKey = $state<Key>("match_score");
  let sortDesc = $state(true);
  let filter = $state("");

  function host(url: string) {
    try {
      return new URL(url).hostname.replace(/^www\./, "");
    } catch {
      return url;
    }
  }

  function setSort(key: Key) {
    if (sortKey === key) sortDesc = !sortDesc;
    else {
      sortKey = key;
      sortDesc = key === "match_score";
    }
  }

  let shown = $derived.by(() => {
    const words = filter.toLowerCase().split(/\s+/).filter(Boolean);
    const list = rows.filter((r) => {
      const hay = `${r.title} ${r.company} ${r.location} ${r.description}`.toLowerCase();
      return words.every((w) => hay.includes(w));
    });
    const dir = sortDesc ? -1 : 1;
    return list.sort((a, b) => {
      if (sortKey === "match_score") return dir * ((a.match_score ?? -1) - (b.match_score ?? -1));
      const x = sortKey === "source" ? host(a.source) : a[sortKey];
      const y = sortKey === "source" ? host(b.source) : b[sortKey];
      return dir * x.localeCompare(y);
    });
  });

  function scoreClass(score: number | null) {
    if (score === null) return "";
    return score >= 75 ? "good" : score >= 50 ? "mid" : "";
  }
</script>

<div class="card table-card">
  <div class="toolbar">
    <div class="title">
      <h2>Results</h2>
      {#if label}
        <span class="muted small">{label} · {rows.length} jobs</span>
      {/if}
    </div>
    {#if rows.length > 0}
      <input type="search" placeholder="Filter" bind:value={filter} aria-label="Filter the results" />
    {/if}
  </div>

  {#if rows.length === 0}
    <div class="empty">
      <p>No results yet.</p>
      <p class="muted small">Upload your CV and click "Find jobs", or open an older CSV file.</p>
    </div>
  {:else}
    <div class="scroll">
      <table>
        <thead>
          <tr>
            {#each columns as col (col.key)}
              <th class:num={col.key === "match_score"}>
                <button onclick={() => setSort(col.key)}>
                  {col.title}
                  {#if sortKey === col.key}<span aria-hidden="true">{sortDesc ? "↓" : "↑"}</span>{/if}
                </button>
              </th>
            {/each}
          </tr>
        </thead>
        <tbody>
          {#each shown as row, i (i)}
            <tr class:selected={row === selected} onclick={() => (selected = row === selected ? null : row)}>
              <td class="num"><span class="score {scoreClass(row.match_score)}">{row.match_score ?? "–"}</span></td>
              <td class="strong">{row.title}</td>
              <td>{row.company}</td>
              <td class="muted">{row.location}</td>
              <td class="muted small">{host(row.source)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
      {#if shown.length === 0}
        <p class="empty muted small">No job matches the filter.</p>
      {/if}
    </div>
  {/if}
</div>

<style>
  .table-card {
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow: hidden;
  }

  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 12px 14px;
    border-bottom: 1px solid var(--border);
  }

  .title {
    display: flex;
    align-items: baseline;
    gap: 10px;
  }

  .toolbar input {
    max-width: 240px;
  }

  .scroll {
    flex: 1;
    overflow: auto;
  }

  table {
    width: 100%;
    border-collapse: collapse;
  }

  th {
    position: sticky;
    top: 0;
    z-index: 1;
    padding: 0;
    background: var(--surface);
    border-bottom: 1px solid var(--border);
    text-align: left;
  }

  th button {
    width: 100%;
    padding: 8px 12px;
    border: 0;
    background: none;
    color: var(--muted);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    text-align: inherit;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    cursor: pointer;
  }

  th button:hover {
    color: var(--text);
  }

  td {
    padding: 9px 12px;
    border-bottom: 1px solid var(--border);
    vertical-align: middle;
  }

  .num {
    width: 72px;
    text-align: center;
  }

  th.num button {
    text-align: center;
  }

  .strong {
    font-weight: 500;
  }

  tbody tr {
    cursor: pointer;
  }

  tbody tr:hover {
    background: var(--surface-2);
  }

  tbody tr.selected {
    background: var(--accent-soft);
  }

  .empty {
    padding: 40px 16px;
    text-align: center;
  }

  .empty p + p {
    margin-top: 4px;
  }
</style>
