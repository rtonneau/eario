// Translation of Eario chart data into an Apache ECharts option.
//
// This is the only place that knows the ECharts option format. The backend
// sends renderer-independent chart data; see docs/adr/0004.

import type { EChartsCoreOption } from 'echarts/core';

import type { ChartData } from './generated/ChartData';

/** Colours of the current theme, read from the design tokens. */
export interface ChartPalette {
  text: string;
  muted: string;
  border: string;
  accent: string;
  surface: string;
}

/** Above this many points, per-point symbols and hover effects are dropped. */
const LARGE_SERIES = 2000;

export function chartOption(chart: ChartData, palette: ChartPalette): EChartsCoreOption {
  const large = chart.points.length > LARGE_SERIES;
  const axis = (name: string) => ({
    type: 'value',
    name,
    nameLocation: 'middle',
    // Do not force the axis to include zero: data far from it would be squashed.
    scale: true,
    nameTextStyle: { color: palette.text, fontWeight: 600 },
    axisLabel: { color: palette.muted },
    axisLine: { lineStyle: { color: palette.border } },
    splitLine: { lineStyle: { color: palette.border, opacity: 0.6 } },
  });

  const series =
    chart.mark === 'line'
      ? {
          type: 'line',
          data: chart.points,
          showSymbol: chart.points.length <= 200,
          symbolSize: 4,
          lineStyle: { width: 1.5 },
        }
      : {
          type: 'scatter',
          data: chart.points,
          symbolSize: large ? 3 : 6,
          large,
        };

  return {
    animation: false,
    color: [palette.accent],
    textStyle: { color: palette.text },
    grid: { left: 64, right: 24, top: 40, bottom: 56 },
    tooltip: {
      trigger: chart.mark === 'line' ? 'axis' : 'item',
      backgroundColor: palette.surface,
      borderColor: palette.border,
      textStyle: { color: palette.text },
    },
    toolbox: {
      right: 16,
      iconStyle: { borderColor: palette.muted },
      feature: {
        dataZoom: { title: { zoom: 'Zoom to a region', back: 'Undo zoom' } },
        restore: { title: 'Reset view' },
        saveAsImage: { title: 'Save as image', name: `${chart.y_label}-vs-${chart.x_label}` },
      },
    },
    dataZoom: [
      { type: 'inside', xAxisIndex: 0, filterMode: 'none' },
      { type: 'inside', yAxisIndex: 0, filterMode: 'none' },
    ],
    xAxis: { ...axis(chart.x_label), nameGap: 30 },
    yAxis: { ...axis(chart.y_label), nameGap: 48 },
    series: [series],
  };
}

/** Sentence describing a chart to people who cannot see it. */
export function chartDescription(chart: ChartData): string {
  const kind = chart.mark === 'line' ? 'Line chart' : 'Scatter plot';
  return `${kind} of ${chart.y_label} against ${chart.x_label}, ${chart.total_points} points.`;
}
