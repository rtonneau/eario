<script lang="ts">
  import { errorMessage } from '../api';
  import type { ComponentSpec } from '../generated/ComponentSpec';
  import { session } from '../session.svelte';

  let { spec }: { spec: ComponentSpec } = $props();

  const kind = $derived(spec.kind.type === 'checkbox' ? spec.kind : null);
  const checked = $derived(kind ? session.state[kind.state] === true : false);
  const inputId = $derived(`checkbox-${spec.id}`);

  let error = $state<string | null>(null);

  function commit(input: HTMLInputElement) {
    if (!kind) {
      return;
    }
    error = null;
    session.setState(kind.state, input.checked).catch((failure: unknown) => {
      error = errorMessage(failure);
      // Back to the value the application holds.
      input.checked = checked;
    });
  }
</script>

<div class="checkbox">
  <input
    id={inputId}
    type="checkbox"
    {checked}
    aria-invalid={error !== null}
    onchange={(event) => commit(event.currentTarget)}
  />
  <label for={inputId}>{kind?.label}</label>
</div>
{#if error !== null}
  <p class="error" role="alert">{error}</p>
{/if}

<style>
  .checkbox {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  input {
    width: 1.125rem;
    height: 1.125rem;
    margin: 0;
    accent-color: var(--color-accent);
  }
</style>
