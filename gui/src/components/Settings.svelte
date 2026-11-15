<script lang="ts">
  // The settings screen. "Save" writes config.yaml, sources.yaml, ignore.yaml and the API keys in .env.
  import { onMount } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import * as api from "../lib/api";
  import { parseExtraBody, type Config, type ProviderConfig, type Rule, type Settings, type Source, type VarValue } from "../lib/types";
  import ProviderTab from "./settings/ProviderTab.svelte";
  import SourcesTab from "./settings/SourcesTab.svelte";
  import SearchTab from "./settings/SearchTab.svelte";
  import IgnoreTab from "./settings/IgnoreTab.svelte";
  import ApplicationsTab from "./settings/ApplicationsTab.svelte";

  interface Props {
    killDups: boolean;
    onClose: () => void;
  }
  let { killDups = $bindable(), onClose }: Props = $props();

  const tabs = [
    { id: "provider", title: "AI provider" },
    { id: "sources", title: "Sources" },
    { id: "search", title: "Search" },
    { id: "ignore", title: "Ignore list" },
    { id: "applications", title: "Applications" },
  ] as const;
  type TabId = (typeof tabs)[number]["id"];

  let tab = $state<TabId>((api.prefs.get("settingsTab") as TabId) ?? "provider");
  let loadError = $state<string | null>(null);
  let saveError = $state<string | null>(null);
  let saving = $state(false);
  let dataDir = $state("");

  // The draft. The variables are a list, so that they can be renamed in place.
  let loaded = $state(false);
  let config = $state<Config>({
    active: "",
    providers: {},
    output_language: "English",
    extra_instructions: "",
    applications_dir: "applications",
    application_instructions: "",
  });
  let vars = $state<{ name: string; value: string }[]>([]);
  let sources = $state<Source[]>([]);
  let rules = $state<Rule[]>([]);
  let apiKeys = $state<Record<string, string>>({});
  let envKeys = $state<string[]>([]);
  let dups = $state(killDups);
  /** The "extra request body" text of each provider, as typed (it can be invalid JSON). */
  let extraTexts = $state<Record<string, string>>({});
  let initial = "";

  function draftText() {
    return JSON.stringify([config, vars, sources, rules, apiKeys, dups, extraTexts]);
  }

  onMount(async () => {
    try {
      const [s, info] = await Promise.all([api.loadSettings(), api.appInfo()]);
      // All optional fields exist, so that the form does not change the draft by itself.
      for (const [name, pc] of Object.entries(s.config.providers)) {
        for (const key of ["base_url", "api_key_env", "max_tokens", "effort", "context_length", "temperature", "timeout"] as const) {
          pc[key] ??= null;
        }
        pc.extra_body ??= null;
        extraTexts[name] = pc.extra_body ? JSON.stringify(pc.extra_body, null, 2) : "";
      }
      for (const r of s.ignore) {
        r.company ??= null;
        r.title ??= null;
        r.url ??= null;
        r.note ??= null;
      }
      config = s.config;
      vars = Object.entries(s.vars).map(([name, value]) => ({ name, value: String(value) }));
      sources = s.sources;
      rules = s.ignore;
      apiKeys = s.api_keys;
      envKeys = s.env_keys;
      dataDir = info.data_dir;
      initial = draftText();
      loaded = true;
    } catch (e) {
      loadError = api.errorText(e);
    }
  });

  $effect(() => {
    api.prefs.set("settingsTab", tab);
  });

  /** Empty text and invalid numbers become "not set". */
  function cleanProvider(p: ProviderConfig): ProviderConfig {
    const text = (v: string | null | undefined) => (v && v.trim() ? v.trim() : null);
    const num = (v: number | null | undefined) => (typeof v === "number" && Number.isFinite(v) ? v : null);
    return {
      type: p.type,
      model: p.model.trim(),
      base_url: text(p.base_url),
      api_key_env: text(p.api_key_env),
      max_tokens: num(p.max_tokens),
      effort: text(p.effort),
      context_length: num(p.context_length),
      temperature: num(p.temperature),
      timeout: num(p.timeout),
      extra_body: p.extra_body ?? null,
    };
  }

  function varsRecord(): Record<string, VarValue> {
    const out: Record<string, VarValue> = {};
    for (const v of vars) {
      const name = v.name.trim();
      if (!name) continue;
      const value = v.value.trim();
      out[name] = /^-?\d+(\.\d+)?$/.test(value) ? Number(value) : value;
    }
    return out;
  }

  async function saveAll() {
    if (!loaded) return;
    saving = true;
    saveError = null;
    const snap = $state.snapshot(config);
    const providers: Record<string, ProviderConfig> = {};
    for (const [name, p] of Object.entries(snap.providers)) {
      const extra = parseExtraBody(extraTexts[name] ?? "");
      if ("error" in extra) {
        saveError = `Provider "${name}", extra request body: ${extra.error}`;
        saving = false;
        return;
      }
      providers[name] = { ...cleanProvider(p), extra_body: extra.value };
    }
    const settings: Settings = {
      config: { ...snap, providers },
      vars: varsRecord(),
      sources: $state.snapshot(sources).map((s) => ({ ...s, name: s.name.trim(), url: s.url.trim() })),
      ignore: $state
        .snapshot(rules)
        .map((r) => ({
          company: r.company?.trim() || null,
          title: r.title?.trim() || null,
          url: r.url?.trim() || null,
          note: r.note?.trim() || null,
        }))
        .filter((r) => r.company || r.title || r.url),
      api_keys: $state.snapshot(apiKeys),
      env_keys: [],
    };
    try {
      await api.saveSettings(settings);
      killDups = dups;
      api.prefs.set("killDups", dups ? "1" : "0");
      onClose();
    } catch (e) {
      saveError = api.errorText(e);
    } finally {
      saving = false;
    }
  }

  async function cancel() {
    if (loaded && draftText() !== initial) {
      const discard = await ask("Your changes are not saved. Do you want to discard them?", {
        title: "Unsaved changes",
        kind: "warning",
        okLabel: "Discard",
        cancelLabel: "Keep editing",
      });
      if (!discard) return;
    }
    onClose();
  }
</script>

<div class="settings">
  <nav>
    <h2>Settings</h2>
    {#each tabs as t (t.id)}
      <button class="tab" class:active={tab === t.id} onclick={() => (tab = t.id)}>{t.title}</button>
    {/each}
    <div class="folder">
      {#if dataDir}
        <span class="hint">Files are in</span>
        <button class="link small" title={dataDir} onclick={() => revealItemInDir(dataDir)}>{dataDir}</button>
      {/if}
    </div>
  </nav>

  <div class="content">
    <div class="scroll">
      {#if loadError}
        <div class="notice error">The settings cannot be read: {loadError}</div>
      {:else if loaded}
        {#if tab === "provider"}
          <ProviderTab bind:config bind:apiKeys bind:extraTexts {envKeys} />
        {:else if tab === "sources"}
          <SourcesTab bind:sources bind:vars varsRecord={varsRecord} />
        {:else if tab === "search"}
          <SearchTab bind:config bind:killDups={dups} />
        {:else if tab === "ignore"}
          <IgnoreTab bind:rules />
        {:else}
          <ApplicationsTab bind:config {dataDir} />
        {/if}
      {/if}
    </div>

    <footer>
      {#if saveError}
        <div class="notice error grow">{saveError}</div>
      {:else}
        <span class="hint grow">Save writes config.yaml, sources.yaml, ignore.yaml and the API keys in .env.</span>
      {/if}
      <button class="btn" onclick={cancel}>Cancel</button>
      <button class="btn btn-primary" onclick={saveAll} disabled={!loaded || saving}>
        {saving ? "Saving ..." : "Save"}
      </button>
    </footer>
  </div>
</div>

<style>
  .settings {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 210px minmax(0, 1fr);
  }

  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 16px 10px;
    border-right: 1px solid var(--border);
    background: var(--surface);
  }

  nav h2 {
    padding: 0 10px 10px;
  }

  .tab {
    padding: 8px 10px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .tab:hover {
    background: var(--surface-2);
  }

  .tab.active {
    background: var(--accent-soft);
    color: var(--accent);
    font-weight: 600;
  }

  .folder {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-top: auto;
    padding: 0 10px;
  }

  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent);
    font: inherit;
    text-align: left;
    word-break: break-all;
    cursor: pointer;
  }

  .content {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .scroll {
    flex: 1;
    overflow: auto;
    padding: 20px 24px;
  }

  footer {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 24px;
    border-top: 1px solid var(--border);
    background: var(--surface);
  }

  .grow {
    flex: 1;
  }

  @media (max-width: 760px) {
    .settings {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: auto minmax(0, 1fr);
    }

    nav {
      flex-direction: row;
      flex-wrap: wrap;
      border-right: 0;
      border-bottom: 1px solid var(--border);
    }

    .folder {
      display: none;
    }
  }
</style>
