<script lang="ts">
  // The ignore list: jobs and companies that are left out of the results.
  import type { Rule } from "../../lib/types";

  interface Props {
    rules: Rule[];
  }
  let { rules = $bindable() }: Props = $props();
</script>

<div class="tab">
  <h2>Ignore list</h2>
  <p class="hint">
    Jobs that match an entry are left out of the results, for example jobs you already applied to.
    A job matches if it matches all the fields that the entry gives. Company and title match at the start of a word,
    and case does not count: "its" matches "ITS GmbH", but not "Digits GmbH".
  </p>

  {#if rules.length > 0}
    <div class="table">
      <div class="head">
        <span>Company</span><span>Title</span><span>Job URL</span><span>Note</span><span></span>
      </div>
      {#each rules as rule, i (i)}
        <div class="rule">
          <input type="text" bind:value={rule.company} placeholder="Any" aria-label="Company" />
          <input type="text" bind:value={rule.title} placeholder="Any" aria-label="Title" />
          <input type="text" bind:value={rule.url} placeholder="Any" aria-label="Job URL" spellcheck="false" />
          <input type="text" bind:value={rule.note} placeholder="For example: applied" aria-label="Note" />
          <button class="btn btn-ghost btn-icon btn-danger" aria-label="Remove" onclick={() => rules.splice(i, 1)}>✕</button>
        </div>
      {/each}
    </div>
  {:else}
    <p class="muted">The list is empty. Use "Ignore" in the job details, or add an entry here.</p>
  {/if}
  <div>
    <button class="btn" onclick={() => rules.push({ company: "", title: "", url: "", note: "" })}>Add entry</button>
  </div>
</div>

<style>
  .tab {
    display: flex;
    flex-direction: column;
    gap: 14px;
    max-width: 1000px;
  }

  .table {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .head,
  .rule {
    display: grid;
    grid-template-columns: 1.2fr 1.2fr 1.6fr 1fr 32px;
    gap: 6px;
    align-items: center;
  }

  .head {
    color: var(--muted);
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }
</style>
