<script lang="ts">
  import { errorMessage } from '../api';
  import type { ComponentSpec } from '../generated/ComponentSpec';
  import { componentData, session } from '../session.svelte';

  let { spec }: { spec: ComponentSpec } = $props();

  const dataset = $derived(spec.kind.type === 'file_import' ? spec.kind.dataset : '');
  const handle = componentData(() => spec.id);
  const info = $derived(handle.data?.type === 'dataset' ? handle.data.info : null);

  let busy = $state(false);
  let error = $state<string | null>(null);
  let dragging = $state(false);

  async function run(action: () => Promise<void>) {
    busy = true;
    error = null;
    try {
      await action();
    } catch (failure) {
      error = errorMessage(failure);
    } finally {
      busy = false;
    }
  }

  function importFile(file: File | undefined) {
    if (file) {
      void run(() => session.upload(dataset, file));
    }
  }

  function onDrop(event: DragEvent) {
    event.preventDefault();
    dragging = false;
    importFile(event.dataTransfer?.files[0]);
  }
</script>

<div
  class="drop"
  class:dragging
  role="group"
  aria-label="File import"
  ondragover={(event) => {
    event.preventDefault();
    dragging = true;
  }}
  ondragleave={() => (dragging = false)}
  ondrop={onDrop}
>
  <label class="field">
    <span>Choose a CSV, TSV or XLSX file, or drop it here</span>
    <input
      type="file"
      accept=".csv,.tsv,.tab,.xlsx,.xlsm"
      disabled={busy}
      onchange={(event) => {
        importFile(event.currentTarget.files?.[0]);
        // Allow importing the same file again after it changed on disk.
        event.currentTarget.value = '';
      }}
    />
  </label>

  {#if busy}
    <p class="muted" role="status">Importing…</p>
  {:else if info}
    <p>Loaded: <strong>{info.source_name}</strong></p>
  {:else}
    <p class="muted">No file loaded yet.</p>
  {/if}

  {#if info && info.sheets.length > 1}
    <label class="field">
      <span>Worksheet</span>
      <select
        value={info.selected_sheet}
        disabled={busy}
        onchange={(event) => {
          const sheet = event.currentTarget.value;
          void run(() => session.selectSheet(dataset, sheet));
        }}
      >
        {#each info.sheets as sheet (sheet)}
          <option value={sheet}>{sheet}</option>
        {/each}
      </select>
    </label>
  {/if}

  {#if error !== null}
    <p class="error" role="alert">{error}</p>
  {/if}
  {#if handle.error !== null}
    <p class="error" role="alert">{handle.error}</p>
  {/if}
</div>

<style>
  .drop {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-3);
    border: 1px dashed var(--color-border);
    border-radius: var(--radius);
  }

  .drop.dragging {
    border-color: var(--color-accent);
    background: var(--color-surface-raised);
  }

  input[type='file'] {
    max-width: 100%;
  }
</style>
