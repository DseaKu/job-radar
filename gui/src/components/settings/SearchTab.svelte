<script lang="ts">
  // How the AI writes and rates the results.
  import type { Config } from "../../lib/types";

  interface Props {
    config: Config;
    killDups: boolean;
  }
  let { config = $bindable(), killDups = $bindable() }: Props = $props();

  const languages = ["English", "German", "French", "Spanish", "Italian", "Dutch", "Polish"];
</script>

<div class="tab">
  <h2>Search</h2>

  <label class="field">
    <span>Language of the results</span>
    <input type="text" bind:value={config.output_language} list="languages" />
    <datalist id="languages">
      {#each languages as l (l)}<option value={l}></option>{/each}
    </datalist>
    <span class="hint">The language of the description and the "why it fits" text. Titles and company names stay as they are on the page.</span>
  </label>

  <label class="field">
    <span>Extra instructions for the AI</span>
    <textarea
      bind:value={config.extra_instructions}
      rows="6"
      placeholder={"For example:\nI look for a working-student job, 20 hours a week. Rate full-time jobs lower.\nRate jobs with Rust or embedded systems higher.\nIgnore jobs in sales and marketing."}
    ></textarea>
    <span class="hint">Write normal sentences. They change how the AI finds and rates the jobs.</span>
  </label>

  <label class="check">
    <input type="checkbox" bind:checked={killDups} />
    <span>
      <strong>Remove duplicates</strong><br />
      <span class="hint">One more AI call merges the same job found on different sources. This setting is stored in the app only.</span>
    </span>
  </label>
</div>

<style>
  .tab {
    display: flex;
    flex-direction: column;
    gap: 18px;
    max-width: 760px;
  }
</style>
