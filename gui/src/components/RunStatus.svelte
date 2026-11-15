<script lang="ts">
  // The progress of a search, with the messages of the library in a collapsible list.
  import type { LogLine } from "../lib/types";

  interface Props {
    running: boolean;
    phase: string;
    current: { index: number; total: number; name: string } | null;
    doneCount: number;
    logs: LogLine[];
  }
  let { running, phase, current, doneCount, logs }: Props = $props();

  let percent = $derived(current && current.total > 0 ? Math.round((doneCount / current.total) * 100) : 0);
  let warnings = $derived(logs.filter((l) => l.level === "warn").length);
  let logBox = $state<HTMLElement | null>(null);

  // Keep the newest message in view.
  $effect(() => {
    logs.length;
    if (logBox) logBox.scrollTop = logBox.scrollHeight;
  });
</script>

<section class="card status">
  {#if running}
    <div class="line">
      <span class="spinner" aria-hidden="true"></span>
      <span class="phase">{phase}</span>
      {#if current}
        <span class="muted small">{Math.min(doneCount + 1, current.total)} of {current.total}</span>
      {/if}
    </div>
    <div class="bar" class:indeterminate={!current}>
      <div style:width={current ? `${percent}%` : undefined}></div>
    </div>
  {/if}
  <details>
    <summary class="small muted">
      Details ({logs.length} messages{warnings > 0 ? `, ${warnings} warnings` : ""})
    </summary>
    <div class="log" bind:this={logBox}>
      {#each logs as line, i (i)}
        <div class:warn={line.level === "warn"}>{line.msg}</div>
      {/each}
    </div>
  </details>
</section>

<style>
  .status {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px 14px;
  }

  .line {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .phase {
    flex: 1;
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .spinner {
    width: 14px;
    height: 14px;
    border: 2px solid var(--accent-soft);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  .bar {
    height: 6px;
    border-radius: 999px;
    background: var(--surface-2);
    overflow: hidden;
  }

  .bar > div {
    height: 100%;
    background: var(--accent);
    border-radius: 999px;
    transition: width 0.3s;
  }

  .bar.indeterminate > div {
    width: 30%;
    animation: slide 1.2s ease-in-out infinite;
  }

  summary {
    cursor: pointer;
    user-select: none;
  }

  .log {
    max-height: 180px;
    margin-top: 8px;
    padding: 8px 10px;
    overflow: auto;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
    font-size: 12px;
    white-space: pre-wrap;
  }

  .log .warn {
    color: var(--danger);
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(340%);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .spinner,
    .bar.indeterminate > div {
      animation: none;
    }
  }
</style>
