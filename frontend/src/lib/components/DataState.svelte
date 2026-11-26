<script lang="ts">
  import type { Snippet } from 'svelte';

  import type { ComponentData } from '../generated/ComponentData';
  import type { ComponentDataHandle } from '../session.svelte';

  // Shows the loading, error and empty states shared by every data-driven
  // component, and hands the data to `children` once there is some.
  let { handle, children }: { handle: ComponentDataHandle; children: Snippet<[ComponentData]> } =
    $props();
</script>

{#if handle.error !== null}
  <p class="error" role="alert">{handle.error}</p>
{:else if handle.data === null}
  <p class="muted">Loading…</p>
{:else if handle.data.type === 'empty'}
  <p class="muted">{handle.data.reason}</p>
{:else}
  {@render children(handle.data)}
{/if}
