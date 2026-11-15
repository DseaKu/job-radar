<script lang="ts">
  // The AI provider: which entry is active, its model and API key, and the advanced fields.
  import { untrack } from "svelte";
  import * as api from "../../lib/api";
  import {
    DEFAULT_BASE_URL,
    DEFAULT_CONTEXT,
    DEFAULT_MODEL,
    KIND_LABEL,
    apiKeyVar,
    type Config,
    type ProviderKind,
  } from "../../lib/types";

  interface Props {
    config: Config;
    apiKeys: Record<string, string>;
    envKeys: string[];
  }
  let { config = $bindable(), apiKeys = $bindable(), envKeys }: Props = $props();

  const kinds: ProviderKind[] = ["ollama", "anthropic", "openai"];
  const efforts: Record<ProviderKind, string[]> = {
    ollama: [],
    anthropic: ["low", "medium", "high", "xhigh", "max"],
    openai: ["low", "medium", "high"],
  };

  let names = $derived(Object.keys(config.providers).sort());
  let p = $derived(config.providers[config.active]);
  let keyVar = $derived(p ? apiKeyVar(p) : null);

  let showKey = $state(false);
  let newName = $state("");
  let addError = $state<string | null>(null);
  let test = $state<{ ok: boolean; text: string } | null>(null);
  let testing = $state(false);
  let models = $state<string[]>([]);
  let modelsError = $state<string | null>(null);

  function setKind(kind: ProviderKind) {
    if (!p) return;
    // A model name of the old type does not work with the new type.
    if (!p.model || p.model === DEFAULT_MODEL[p.type]) p.model = DEFAULT_MODEL[kind];
    if (!efforts[kind].includes(p.effort ?? "") && kind !== "ollama") p.effort = null;
    p.type = kind;
    test = null;
  }

  function addProvider() {
    const name = newName.trim();
    addError = null;
    if (!/^[A-Za-z0-9_-]+$/.test(name)) {
      addError = "Use letters, digits, - and _ only.";
      return;
    }
    if (config.providers[name]) {
      addError = `"${name}" already exists.`;
      return;
    }
    config.providers[name] = { type: "anthropic", model: DEFAULT_MODEL.anthropic };
    config.active = name;
    newName = "";
  }

  function removeProvider() {
    if (names.length < 2) return;
    delete config.providers[config.active];
    config.active = Object.keys(config.providers).sort()[0];
  }

  async function testConnection() {
    testing = true;
    test = null;
    try {
      const text = await api.testConnection($state.snapshot(config), $state.snapshot(apiKeys));
      test = { ok: true, text };
    } catch (e) {
      test = { ok: false, text: api.errorText(e) };
    } finally {
      testing = false;
    }
  }

  async function loadModels() {
    modelsError = null;
    try {
      models = await api.ollamaModels(p?.base_url ?? null);
      if (models.length === 0) modelsError = "Ollama has no models. Install one with: ollama pull qwen3:1.7b";
    } catch (e) {
      models = [];
      modelsError = api.errorText(e);
    }
  }

  // Load the installed models when a local provider is shown (not again for each key press).
  $effect(() => {
    if (p?.type === "ollama") untrack(loadModels);
  });
</script>

<div class="tab">
  <section>
    <h2>Provider</h2>
    <p class="hint">The AI that reads the job pages and rates the jobs. A local model is free; a cloud model is better and needs an API key.</p>
    <div class="row">
      <label class="field grow">
        <span>Active provider</span>
        <select bind:value={config.active} onchange={() => (test = null)}>
          {#each names as name (name)}
            <option value={name}>{name} – {KIND_LABEL[config.providers[name].type]}</option>
          {/each}
        </select>
      </label>
      <button class="btn btn-danger" onclick={removeProvider} disabled={names.length < 2}>Remove</button>
    </div>
    <div class="row">
      <input type="text" placeholder="Name of a new provider entry" bind:value={newName} onkeydown={(e) => e.key === "Enter" && addProvider()} />
      <button class="btn" onclick={addProvider} disabled={!newName.trim()}>Add</button>
    </div>
    {#if addError}<p class="small error-text">{addError}</p>{/if}
  </section>

  {#if p}
    <section class="card box">
      <div class="grid">
        <label class="field">
          <span>Type</span>
          <select value={p.type} onchange={(e) => setKind(e.currentTarget.value as ProviderKind)}>
            {#each kinds as kind (kind)}
              <option value={kind}>{KIND_LABEL[kind]}</option>
            {/each}
          </select>
        </label>
        <label class="field">
          <span>Model</span>
          <input type="text" bind:value={p.model} list="model-list" placeholder={DEFAULT_MODEL[p.type]} />
          <datalist id="model-list">
            {#if p.type === "ollama"}
              {#each models as m (m)}<option value={m}></option>{/each}
            {:else if p.type === "anthropic"}
              <option value="claude-haiku-5-5">Fastest and cheapest</option>
              <option value="claude-sonnet-5-5">Balanced</option>
              <option value="claude-opus-5-5">Most capable</option>
            {/if}
          </datalist>
          {#if p.type === "ollama"}
            {#if modelsError}
              <span class="hint error-text">{modelsError}</span>
            {:else if models.length > 0}
              <span class="hint">Installed: {models.join(", ")}</span>
            {/if}
          {/if}
        </label>
      </div>

      {#if keyVar}
        <label class="field">
          <span>API key</span>
          <div class="row">
            <input
              type={showKey ? "text" : "password"}
              bind:value={apiKeys[keyVar]}
              placeholder={envKeys.includes(keyVar) ? "Set in the environment of the app" : "Paste your API key"}
              autocomplete="off"
              spellcheck="false"
            />
            <button class="btn" onclick={() => (showKey = !showKey)}>{showKey ? "Hide" : "Show"}</button>
          </div>
          <span class="hint">Saved in the .env file as {keyVar}.</span>
        </label>
      {:else}
        <p class="hint">A local model needs no API key. Ollama must run on this computer.</p>
      {/if}

      <div class="row test">
        <button class="btn" onclick={testConnection} disabled={testing}>
          {testing ? "Testing ..." : "Test connection"}
        </button>
        {#if test}
          <span class="small" class:ok-text={test.ok} class:error-text={!test.ok}>{test.text}</span>
        {/if}
      </div>

      <details>
        <summary>Advanced</summary>
        <div class="grid">
          <label class="field">
            <span>Base URL</span>
            <input type="text" bind:value={p.base_url} placeholder={DEFAULT_BASE_URL[p.type]} />
          </label>
          <label class="field">
            <span>Thinking effort</span>
            {#if p.type === "ollama"}
              <select value={p.effort ? "on" : ""} onchange={(e) => p && (p.effort = e.currentTarget.value || null)}>
                <option value="">Off (faster)</option>
                <option value="on">On</option>
              </select>
            {:else}
              <select bind:value={p.effort}>
                <option value={null}>Model default</option>
                {#each efforts[p.type] as level (level)}<option value={level}>{level}</option>{/each}
              </select>
            {/if}
          </label>
          <label class="field">
            <span>Max tokens per reply</span>
            <input type="number" min="256" step="256" bind:value={p.max_tokens} placeholder="4096" />
          </label>
          <label class="field">
            <span>Context length</span>
            <input type="number" min="2048" step="1024" bind:value={p.context_length} placeholder={String(DEFAULT_CONTEXT[p.type])} />
          </label>
          <label class="field">
            <span>Temperature</span>
            <input type="number" min="0" max="2" step="0.1" bind:value={p.temperature} placeholder="Provider default" />
          </label>
          {#if p.type !== "ollama"}
            <label class="field">
              <span>API key variable</span>
              <input type="text" bind:value={p.api_key_env} placeholder={apiKeyVar({ ...p, api_key_env: null }) ?? ""} />
            </label>
          {/if}
        </div>
        <p class="hint">
          Empty fields use the defaults. A page that is longer than the context length is read in parts.
          A long page with many jobs needs more tokens per reply.
        </p>
      </details>
    </section>
  {/if}
</div>

<style>
  .tab {
    display: flex;
    flex-direction: column;
    gap: 18px;
    max-width: 760px;
  }

  section {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .box {
    gap: 14px;
    padding: 16px;
  }

  .row {
    display: flex;
    align-items: flex-end;
    gap: 8px;
  }

  .row.test {
    align-items: center;
  }

  .grow {
    flex: 1;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
    gap: 12px;
  }

  details {
    border-top: 1px solid var(--border);
    padding-top: 10px;
  }

  details[open] > .grid {
    margin: 12px 0 8px;
  }

  summary {
    font-weight: 500;
    cursor: pointer;
    user-select: none;
  }
</style>
