<script lang="ts">
  import { formatBytes, formatNumber } from '../format';
  import type { ComponentSpec } from '../generated/ComponentSpec';
  import { componentData } from '../session.svelte';
  import DataState from './DataState.svelte';

  let { spec }: { spec: ComponentSpec } = $props();

  const handle = componentData(() => spec.id);
</script>

<DataState {handle}>
  {#snippet children(data)}
    {#if data.type === 'dataset'}
      {@const info = data.info}
      <dl>
        <div><dt>Source</dt><dd>{info.source_name}</dd></div>
        {#if info.selected_sheet !== null}
          <div><dt>Worksheet</dt><dd>{info.selected_sheet}</dd></div>
        {/if}
        <div><dt>Format</dt><dd>{info.format.toUpperCase()}</dd></div>
        <div><dt>Rows</dt><dd>{formatNumber(info.rows)}</dd></div>
        <div><dt>Columns</dt><dd>{formatNumber(info.columns.length)}</dd></div>
        <div><dt>File size</dt><dd>{formatBytes(info.source_bytes)}</dd></div>
      </dl>
      <!-- A scrolling region must be reachable by keyboard to scroll it (WCAG 2.1.1);
           the region role and label make the focus stop meaningful. -->
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <div class="scroll-x columns" role="region" aria-label="Columns" tabindex="0">
        <table>
          <thead>
            <tr>
              <th scope="col">Column</th>
              <th scope="col">Kind</th>
              <th scope="col">Storage type</th>
              <th scope="col" class="number">Missing</th>
            </tr>
          </thead>
          <tbody>
            {#each info.columns as column (column.name)}
              <tr>
                <td>{column.name}</td>
                <td>{column.kind}</td>
                <td class="muted">{column.dtype}</td>
                <td class="number">{formatNumber(column.null_count)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  {/snippet}
</DataState>

<style>
  dl {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-6);
    margin: 0;
  }

  dt {
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--color-text-muted);
  }

  dd {
    margin: 0;
    font-weight: 600;
  }

  .columns {
    max-height: 14rem;
    overflow-y: auto;
  }

  thead th {
    position: sticky;
    top: 0;
  }
</style>
