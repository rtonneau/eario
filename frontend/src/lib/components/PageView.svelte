<script lang="ts">
  import type { PageSpec } from '../generated/PageSpec';
  import ComponentView from './ComponentView.svelte';

  let { page }: { page: PageSpec } = $props();
</script>

<div class="page">
  {#each page.rows as row, index (index)}
    <div class="row">
      {#each row.items as component (component.id)}
        <div class="item" style:--span={component.span}>
          <ComponentView spec={component} />
        </div>
      {/each}
    </div>
  {/each}
</div>

<style>
  .page {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .row {
    display: grid;
    grid-template-columns: repeat(12, minmax(0, 1fr));
    gap: var(--space-4);
  }

  .item {
    grid-column: span var(--span);
    min-width: 0;
  }

  /* On narrow screens every component takes the full width. */
  @media (max-width: 860px) {
    .item {
      grid-column: 1 / -1;
    }
  }
</style>
