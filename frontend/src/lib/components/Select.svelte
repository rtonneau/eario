<script lang="ts">
  import { errorMessage } from '../api';
  import { formatNumber } from '../format';
  import type { ComponentSpec } from '../generated/ComponentSpec';
  import { componentData, session } from '../session.svelte';

  let { spec }: { spec: ComponentSpec } = $props();

  const kind = $derived(spec.kind.type === 'select' ? spec.kind : null);
  const handle = componentData(() => spec.id);
  const offered = $derived(handle.data?.type === 'select' ? handle.data : null);
  const options = $derived(offered?.options ?? []);
  const value = $derived.by(() => {
    const current = kind ? session.state[kind.state] : null;
    return typeof current === 'string' ? current : '';
  });
  const inputId = $derived(`select-${spec.id}`);

  let error = $state<string | null>(null);

  function choose(select: HTMLSelectElement) {
    if (!kind) {
      return;
    }
    error = null;
    session.setState(kind.state, select.value).catch((failure: unknown) => {
      error = errorMessage(failure);
      // Back to the value the application holds.
      select.value = value;
    });
  }
</script>

<div class="field">
  <label for={inputId}>{kind?.label}</label>
  <select
    id={inputId}
    {value}
    disabled={options.length === 0}
    aria-invalid={error !== null}
    onchange={(event) => choose(event.currentTarget)}
  >
    {#if options.length === 0}
      <option value="">No value available</option>
    {/if}
    {#each options as option (option)}
      <option value={option}>{option}</option>
    {/each}
  </select>
  {#if error !== null}
    <p class="error" role="alert">{error}</p>
  {:else if handle.error !== null}
    <p class="error" role="alert">{handle.error}</p>
  {:else if handle.data?.type === 'empty'}
    <p class="muted">{handle.data.reason}</p>
  {:else if offered?.truncated}
    <p class="muted" role="status">
      Only the first {formatNumber(options.length)} values are offered.
    </p>
  {/if}
</div>
