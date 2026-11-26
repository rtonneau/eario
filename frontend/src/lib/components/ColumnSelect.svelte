<script lang="ts">
  import { errorMessage } from '../api';
  import type { ComponentSpec } from '../generated/ComponentSpec';
  import { componentData, session } from '../session.svelte';

  let { spec }: { spec: ComponentSpec } = $props();

  const kind = $derived(spec.kind.type === 'column_select' ? spec.kind : null);
  const handle = componentData(() => spec.id);
  const options = $derived(handle.data?.type === 'column_select' ? handle.data.options : []);
  const value = $derived.by(() => {
    const current = kind ? session.state[kind.state] : null;
    return typeof current === 'string' ? current : '';
  });
  const inputId = $derived(`column-${spec.id}`);

  function choose(column: string) {
    if (kind) {
      session.setState(kind.state, column === '' ? null : column).catch((failure: unknown) => {
        session.notify('error', errorMessage(failure));
      });
    }
  }
</script>

<div class="field">
  <label for={inputId}>{kind?.label}</label>
  <select
    id={inputId}
    {value}
    disabled={options.length === 0}
    onchange={(event) => choose(event.currentTarget.value)}
  >
    {#if options.length === 0}
      <option value="">No column available</option>
    {/if}
    {#each options as option (option)}
      <option value={option}>{option}</option>
    {/each}
  </select>
  {#if handle.error !== null}
    <p class="error" role="alert">{handle.error}</p>
  {/if}
</div>
