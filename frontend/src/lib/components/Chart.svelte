<script lang="ts">
  import { LineChart, ScatterChart } from 'echarts/charts';
  import {
    DataZoomComponent,
    GridComponent,
    ToolboxComponent,
    TooltipComponent,
  } from 'echarts/components';
  import { type ECharts, init, use } from 'echarts/core';
  import { CanvasRenderer } from 'echarts/renderers';

  import { type ChartPalette, chartDescription, chartOption } from '../chart-option';
  import { formatNumber } from '../format';
  import type { ComponentSpec } from '../generated/ComponentSpec';
  import { componentData, session } from '../session.svelte';
  import DataState from './DataState.svelte';

  // Only the parts of ECharts this component uses end up in the bundle.
  use([
    LineChart,
    ScatterChart,
    GridComponent,
    TooltipComponent,
    DataZoomComponent,
    ToolboxComponent,
    CanvasRenderer,
  ]);

  let { spec }: { spec: ComponentSpec } = $props();

  const handle = componentData(() => spec.id);
  const chart = $derived(handle.data?.type === 'chart' ? handle.data.chart : null);

  let container = $state<HTMLDivElement | null>(null);
  let instance = $state.raw<ECharts | null>(null);

  function palette(element: HTMLElement): ChartPalette {
    const styles = getComputedStyle(element);
    const token = (name: string) => styles.getPropertyValue(name).trim();
    return {
      text: token('--color-text'),
      muted: token('--color-text-muted'),
      border: token('--color-border'),
      accent: token('--color-accent'),
      surface: token('--color-surface'),
    };
  }

  // Create the chart when its container appears, and dispose of it after.
  $effect(() => {
    const element = container;
    if (element === null) {
      return;
    }
    const created = init(element);
    instance = created;
    const observer = new ResizeObserver(() => created.resize());
    observer.observe(element);
    return () => {
      observer.disconnect();
      created.dispose();
      instance = null;
    };
  });

  // Redraw when the data or the theme changes.
  $effect(() => {
    void session.theme;
    if (container !== null && instance !== null && chart !== null) {
      instance.setOption(chartOption(chart, palette(container)), true);
    }
  });
</script>

<DataState {handle}>
  {#snippet children(data)}
    {#if data.type === 'chart'}
      <div
        class="chart"
        bind:this={container}
        role="img"
        aria-label={chartDescription(data.chart)}
      ></div>
      <p class="muted">
        {#if data.chart.total_points === 0}
          No point to draw.
        {:else if data.chart.points.length < data.chart.total_points}
          Showing {formatNumber(data.chart.points.length)} of
          {formatNumber(data.chart.total_points)} points, evenly sampled.
        {:else}
          {formatNumber(data.chart.total_points)} points.
        {/if}
        {#if data.chart.skipped_rows > 0}
          {formatNumber(data.chart.skipped_rows)} rows skipped for missing values.
        {/if}
        Scroll to zoom, drag to pan.
      </p>
    {/if}
  {/snippet}
</DataState>

<style>
  .chart {
    width: 100%;
    height: 26rem;
  }
</style>
