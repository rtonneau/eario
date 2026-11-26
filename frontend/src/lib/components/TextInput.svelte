<script lang="ts">
  import { errorMessage } from '../api';
  import type { ComponentSpec } from '../generated/ComponentSpec';
  import { session } from '../session.svelte';

  let { spec }: { spec: ComponentSpec } = $props();

  const kind = $derived(spec.kind.type === 'text_input' ? spec.kind : null);
  const value = $derived.by(() => {
    const current = kind ? session.state[kind.state] : null;
    return typeof current === 'string' ? current : '';
  });
  const inputId = $derived(`text-${spec.id}`);
  const hintId = $derived(`text-hint-${spec.id}`);

  let error = $state<string | null>(null);

  function commit(input: HTMLInputElement) {
    if (!kind) {
      return;
    }
    const text = input.value;
    // Counted in characters, as the application counts them. The
    // `maxlength` attribute would count UTF-16 units and refuse valid text.
    const length = [...text].length;
    if (length > kind.max_length) {
      error = `Enter at most ${kind.max_length} characters; this text has ${length}.`;
      input.value = value;
      return;
    }
    error = null;
    session.setState(kind.state, text).catch((failure: unknown) => {
      error = errorMessage(failure);
      // Back to the value the application holds.
      input.value = value;
    });
  }
</script>

<div class="field">
  <label for={inputId}>{kind?.label}</label>
  <input
    id={inputId}
    type="text"
    {value}
    aria-describedby={hintId}
    aria-invalid={error !== null}
    onchange={(event) => commit(event.currentTarget)}
  />
  <p id={hintId} class="muted hint">At most {kind?.max_length} characters.</p>
  {#if error !== null}
    <p class="error" role="alert">{error}</p>
  {/if}
</div>

<style>
  .hint {
    font-size: 0.8125rem;
  }
</style>
