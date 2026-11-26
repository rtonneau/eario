<script lang="ts">
  import { errorMessage } from '../api';
  import { formatNumber } from '../format';
  import type { ComponentSpec } from '../generated/ComponentSpec';
  import { session } from '../session.svelte';

  let { spec }: { spec: ComponentSpec } = $props();

  const kind = $derived(spec.kind.type === 'number_input' ? spec.kind : null);
  const value = $derived.by(() => {
    const current = kind ? session.state[kind.state] : null;
    return typeof current === 'number' ? current : null;
  });
  const inputId = $derived(`number-${spec.id}`);
  const hintId = $derived(`number-hint-${spec.id}`);
  const hint = $derived.by(() => {
    if (!kind) {
      return '';
    }
    const what = kind.integer ? 'A whole number' : 'A number';
    if (kind.min !== null && kind.max !== null) {
      return `${what} from ${formatNumber(kind.min)} to ${formatNumber(kind.max)}.`;
    }
    if (kind.min !== null) {
      return `${what} of at least ${formatNumber(kind.min)}.`;
    }
    if (kind.max !== null) {
      return `${what} of at most ${formatNumber(kind.max)}.`;
    }
    return `${what}.`;
  });

  let error = $state<string | null>(null);

  // The field goes back to the value the application holds when the typed
  // one is refused; the alert says why.
  function restore(input: HTMLInputElement) {
    input.value = value === null ? '' : String(value);
  }

  function commit(input: HTMLInputElement) {
    if (!kind) {
      return;
    }
    const text = input.value;
    const number = Number(text);
    if (text.trim() === '' || !Number.isFinite(number)) {
      error = 'Enter a number.';
      restore(input);
      return;
    }
    if (kind.integer && !Number.isInteger(number)) {
      error = 'Enter a whole number.';
      restore(input);
      return;
    }
    error = null;
    session.setState(kind.state, number).catch((failure: unknown) => {
      error = errorMessage(failure);
      restore(input);
    });
  }
</script>

<div class="field">
  <label for={inputId}>{kind?.label}</label>
  <input
    id={inputId}
    type="number"
    inputmode={kind?.integer ? 'numeric' : 'decimal'}
    min={kind?.min ?? undefined}
    max={kind?.max ?? undefined}
    step={kind?.step ?? (kind?.integer ? 1 : 'any')}
    value={value ?? ''}
    aria-describedby={hintId}
    aria-invalid={error !== null}
    onchange={(event) => commit(event.currentTarget)}
  />
  <p id={hintId} class="muted hint">{hint}</p>
  {#if error !== null}
    <p class="error" role="alert">{error}</p>
  {/if}
</div>

<style>
  .hint {
    font-size: 0.8125rem;
  }
</style>
