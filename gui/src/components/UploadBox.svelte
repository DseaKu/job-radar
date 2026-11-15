<script lang="ts">
  // The CV upload: a drop zone that also opens a file dialog on click.
  // The drop itself comes from the window (see App.svelte), because only Tauri knows the file path.
  interface Props {
    path: string | null;
    words: number | null;
    error: string | null;
    dragging: boolean;
    disabled: boolean;
    onChoose: () => void;
  }
  let { path, words, error, dragging, disabled, onChoose }: Props = $props();

  let name = $derived(path ? (path.split(/[\\/]/).pop() ?? path) : null);
</script>

<button class="drop" class:dragging class:has-file={!!name} {disabled} onclick={onChoose} title={path ?? ""}>
  <svg viewBox="0 0 24 24" width="28" height="28" aria-hidden="true">
    <path
      fill="none"
      stroke="currentColor"
      stroke-width="1.6"
      stroke-linecap="round"
      stroke-linejoin="round"
      d="M14 3H7a2 2 0 00-2 2v14a2 2 0 002 2h10a2 2 0 002-2V8l-5-5zM14 3v5h5M12 18v-6M9.5 14.5L12 12l2.5 2.5"
    />
  </svg>
  <div class="text">
    {#if name}
      <strong>{name}</strong>
      {#if error}
        <span class="small error-text">{error}</span>
      {:else if words !== null}
        <span class="small muted">{words.toLocaleString()} words · click or drop a file to change it</span>
      {:else}
        <span class="small muted">Reading ...</span>
      {/if}
    {:else}
      <strong>Drop your CV here</strong>
      <span class="small muted">or click to select a file (PDF or text)</span>
    {/if}
  </div>
</button>

<style>
  .drop {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 14px;
    min-height: 76px;
    padding: 14px 18px;
    border: 1.5px dashed var(--border-strong);
    border-radius: var(--radius);
    background: var(--surface-2);
    color: var(--muted);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition: border-color 0.12s, background 0.12s;
  }

  .drop:hover:not(:disabled),
  .drop.dragging {
    border-color: var(--accent);
    background: var(--accent-soft);
    color: var(--accent);
  }

  .drop.has-file {
    border-style: solid;
    color: var(--accent);
  }

  .drop:disabled {
    cursor: default;
    opacity: 0.7;
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    color: var(--text);
  }

  .text strong {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
