<script lang="ts">
  import { formatNumber } from '../format';
  import type { ComponentSpec } from '../generated/ComponentSpec';
  import { componentData } from '../session.svelte';
  import DataState from './DataState.svelte';

  let { spec }: { spec: ComponentSpec } = $props();

  const handle = componentData(() => spec.id);
</script>

<DataState {handle}>
  {#snippet children(data)}
    {#if data.type === 'statistics'}
      {#if data.stats.columns.length === 0}
        <p class="muted">The dataset has no numeric column.</p>
      {:else}
        <!-- A scrolling region must be reachable by keyboard to scroll it (WCAG 2.1.1);
             the region role and label make the focus stop meaningful. -->
        <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
        <div class="scroll-x" role="region" aria-label="Statistics" tabindex="0">
          <table>
            <caption class="visually-hidden">
              Descriptive statistics of {formatNumber(data.stats.rows)} rows
            </caption>
            <thead>
              <tr>
                <th scope="col">Column</th>
                <th scope="col" class="number">Count</th>
                <th scope="col" class="number">Missing</th>
                <th scope="col" class="number">Mean</th>
                <th scope="col" class="number">Std. dev.</th>
                <th scope="col" class="number">Min</th>
                <th scope="col" class="number">Median</th>
                <th scope="col" class="number">Max</th>
              </tr>
            </thead>
            <tbody>
              {#each data.stats.columns as column (column.column)}
                <tr>
                  <th scope="row">{column.column}</th>
                  <td class="number">{formatNumber(column.count)}</td>
                  <td class="number">{formatNumber(column.null_count)}</td>
                  <td class="number">{formatNumber(column.mean)}</td>
                  <td class="number">{formatNumber(column.std)}</td>
                  <td class="number">{formatNumber(column.min)}</td>
                  <td class="number">{formatNumber(column.median)}</td>
                  <td class="number">{formatNumber(column.max)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
        <p class="muted">Computed over {formatNumber(data.stats.rows)} rows.</p>
      {/if}
    {/if}
  {/snippet}
</DataState>

<style>
  tbody th {
    color: var(--color-text);
  }
</style>
