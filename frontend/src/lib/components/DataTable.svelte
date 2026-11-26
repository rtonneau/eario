<script lang="ts">
  import { formatCell, formatNumber } from '../format';
  import type { ComponentSpec } from '../generated/ComponentSpec';
  import { componentData, session } from '../session.svelte';
  import DataState from './DataState.svelte';

  let { spec }: { spec: ComponentSpec } = $props();

  const pageSize = $derived(spec.kind.type === 'data_table' ? spec.kind.page_size : 50);
  let offset = $state(0);

  // New data (another file, sheet or filter) starts again from the first row.
  $effect(() => {
    void session.versions[spec.id];
    offset = 0;
  });

  const handle = componentData(
    () => spec.id,
    () => offset,
  );
</script>

<DataState {handle}>
  {#snippet children(data)}
    {#if data.type === 'data_table'}
      {@const table = data.table}
      {@const last = table.offset + table.rows.length}
      <div class="scroll-x rows">
        <table>
          <thead>
            <tr>
              <th scope="col" class="number">#</th>
              {#each table.columns as column (column)}
                <th scope="col">{column}</th>
              {/each}
            </tr>
          </thead>
          <tbody>
            {#each table.rows as row, index (table.offset + index)}
              <tr>
                <td class="number muted">{formatNumber(table.offset + index + 1)}</td>
                {#each row as cell, cellIndex (cellIndex)}
                  <td class:number={typeof cell === 'number'} class:muted={cell === null}>
                    {formatCell(cell)}
                  </td>
                {/each}
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      <div class="pager">
        <p class="muted" role="status">
          {#if table.total_rows === 0}
            No rows match.
          {:else}
            Rows {formatNumber(table.offset + 1)}–{formatNumber(last)} of
            {formatNumber(table.total_rows)}
          {/if}
          {#if table.total_rows !== table.unfiltered_rows}
            (filtered from {formatNumber(table.unfiltered_rows)})
          {/if}
        </p>
        <button
          disabled={table.offset === 0}
          onclick={() => (offset = Math.max(0, offset - pageSize))}
        >
          Previous
        </button>
        <button disabled={last >= table.total_rows} onclick={() => (offset += pageSize)}>
          Next
        </button>
      </div>
    {/if}
  {/snippet}
</DataState>

<style>
  .rows {
    max-height: 32rem;
    overflow-y: auto;
    border: 1px solid var(--color-border);
    border-radius: 6px;
  }

  thead th {
    position: sticky;
    top: 0;
  }

  tbody tr:hover {
    background: var(--color-surface-raised);
  }

  .pager {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .pager p {
    flex: 1;
  }
</style>
