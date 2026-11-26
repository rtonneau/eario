<script lang="ts">
  import { errorMessage } from '../api';
  import { formatNumber } from '../format';
  import type { ComponentSpec } from '../generated/ComponentSpec';
  import { session } from '../session.svelte';

  let { spec }: { spec: ComponentSpec } = $props();

  const kind = $derived(spec.kind.type === 'slider' ? spec.kind : null);
  const value = $derived.by(() => {
    const current = kind ? session.state[kind.state] : null;
    return typeof current === 'number' ? current : (kind?.min ?? 0);
  });
  const inputId = $derived(`slider-${spec.id}`);

  // The position while the user drags, before the value is sent.
  let dragged = $state<number | null>(null);
  let error = $state<string | null>(null);

  function commit(input: HTMLInputElement) {
    if (!kind) {
      return;
    }
    const number = Number(input.value);
    error = null;
    session
      .setState(kind.state, number)
      .catch((failure: unknown) => {
        error = errorMessage(failure);
        // Back to the value the application holds.
        input.value = String(value);
      })
      .finally(() => {
        dragged = null;
      });
  }
</script>

<div class="field">
  <label for={inputId}>{kind?.label}</label>
  <div class="track">
    <input
      id={inputId}
      type="range"
      min={kind?.min}
      max={kind?.max}
      step={kind?.step}
      {value}
      aria-invalid={error !== null}
      oninput={(event) => (dragged = Number(event.currentTarget.value))}
      onchange={(event) => commit(event.currentTarget)}
    />
    <output for={inputId}>{formatNumber(dragged ?? value)}</output>
  </div>
  {#if error !== null}
    <p class="error" role="alert">{error}</p>
  {/if}
</div>

<style>
  .track {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  input {
    flex: 1;
    min-width: 0;
    accent-color: var(--color-accent);
  }

  output {
    min-width: 3rem;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
</style>
