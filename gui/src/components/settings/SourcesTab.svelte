<script lang="ts">
  // The search pages and the variables for their URLs.
  import * as api from "../../lib/api";
  import type { PageTest, Source, VarValue } from "../../lib/types";

  interface Props {
    sources: Source[];
    vars: { name: string; value: string }[];
    varsRecord: () => Record<string, VarValue>;
  }
  let { sources = $bindable(), vars = $bindable(), varsRecord }: Props = $props();

  // The test results, by the position of the source.
  let tests = $state<Record<number, { busy: boolean; result?: PageTest; error?: string }>>({});

  async function testSource(i: number) {
    tests[i] = { busy: true };
    try {
      tests[i] = { busy: false, result: await api.testPage(sources[i].url, varsRecord()) };
    } catch (e) {
      tests[i] = { busy: false, error: api.errorText(e) };
    }
  }

  function removeSource(i: number) {
    sources.splice(i, 1);
    tests = {};
  }

  function move(i: number, by: number) {
    const j = i + by;
    if (j < 0 || j >= sources.length) return;
    [sources[i], sources[j]] = [sources[j], sources[i]];
    tests = {};
  }
</script>

<div class="tab">
  <section>
    <h2>Sources</h2>
    <p class="hint">
      The job search pages to read. Do the search in your browser, then copy the URL from the address bar.
      Pages that build their job list with JavaScript (for example LinkedIn or Indeed) give no jobs: use "Test" to check a page without AI cost.
    </p>

    {#each sources as source, i (i)}
      <div class="card source" class:off={!source.enabled}>
        <div class="line">
          <label class="check" title="Read this source">
            <input type="checkbox" bind:checked={source.enabled} />
          </label>
          <input class="name" type="text" bind:value={source.name} placeholder="Name" aria-label="Name" />
          <div class="buttons">
            <button class="btn btn-ghost btn-icon" aria-label="Move up" onclick={() => move(i, -1)} disabled={i === 0}>↑</button>
            <button class="btn btn-ghost btn-icon" aria-label="Move down" onclick={() => move(i, 1)} disabled={i === sources.length - 1}>↓</button>
            <button class="btn" onclick={() => testSource(i)} disabled={!source.url.trim() || tests[i]?.busy}>
              {tests[i]?.busy ? "Testing ..." : "Test"}
            </button>
            <button class="btn btn-ghost btn-icon btn-danger" aria-label="Remove" onclick={() => removeSource(i)}>✕</button>
          </div>
        </div>
        <input type="text" bind:value={source.url} placeholder="https://..." aria-label="URL" spellcheck="false" />
        {#if tests[i]?.error}
          <p class="small error-text">{tests[i].error}</p>
        {:else if tests[i]?.result}
          {@const r = tests[i].result!}
          <details>
            <summary class="small" class:ok-text={r.chars > 500} class:error-text={r.chars <= 500}>
              {r.chars.toLocaleString()} characters of text{r.chars <= 500 ? " – probably no job list (the page needs JavaScript?)" : ""}
            </summary>
            <pre>{r.preview}</pre>
          </details>
        {/if}
      </div>
    {/each}
    <div>
      <button class="btn" onclick={() => sources.push({ name: "", url: "", enabled: true })}>Add source</button>
    </div>
  </section>

  <section>
    <h2>Variables</h2>
    <p class="hint">
      Write <code>{"{{ name }}"}</code> in a URL to insert the value of a variable, for example a search radius for all pages.
    </p>
    {#each vars as v, i (i)}
      <div class="var">
        <input type="text" bind:value={v.name} placeholder="Name" aria-label="Variable name" />
        <input type="text" bind:value={v.value} placeholder="Value" aria-label="Variable value" />
        <button class="btn btn-ghost btn-icon btn-danger" aria-label="Remove" onclick={() => vars.splice(i, 1)}>✕</button>
      </div>
    {/each}
    <div>
      <button class="btn" onclick={() => vars.push({ name: "", value: "" })}>Add variable</button>
    </div>
  </section>
</div>

<style>
  .tab {
    display: flex;
    flex-direction: column;
    gap: 26px;
    max-width: 900px;
  }

  section {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .source {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 12px;
  }

  .source.off {
    opacity: 0.6;
  }

  .line {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .name {
    flex: 1;
    font-weight: 500;
  }

  .buttons {
    display: flex;
    gap: 4px;
  }

  .var {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 2fr) auto;
    gap: 8px;
    max-width: 560px;
  }

  summary {
    cursor: pointer;
  }

  pre {
    max-height: 200px;
    margin: 8px 0 0;
    padding: 8px 10px;
    overflow: auto;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    font-size: 12px;
    white-space: pre-wrap;
  }

  code {
    padding: 1px 4px;
    border-radius: 4px;
    background: var(--surface-2);
  }
</style>
