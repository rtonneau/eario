import { describe, expect, it } from 'vitest';

import { type ChartPalette, chartDescription, chartOption } from './chart-option';
import type { ChartData } from './generated/ChartData';

const palette: ChartPalette = {
  text: '#111111',
  muted: '#666666',
  border: '#cccccc',
  accent: '#0b7577',
  surface: '#ffffff',
};

function chart(overrides: Partial<ChartData> = {}): ChartData {
  return {
    mark: 'scatter',
    x_label: 'energy_keV',
    y_label: 'counts',
    points: [
      [500, 48],
      [525, 69],
    ],
    total_points: 2,
    skipped_rows: 0,
    ...overrides,
  };
}

// The option is a loosely typed object; tests read it as plain data.
function plain(option: unknown): Record<string, any> {
  return option as Record<string, any>;
}

describe('chartOption', () => {
  it('puts the column names on the axes and the points in one series', () => {
    const option = plain(chartOption(chart(), palette));
    expect(option.xAxis.name).toBe('energy_keV');
    expect(option.yAxis.name).toBe('counts');
    expect(option.series).toHaveLength(1);
    expect(option.series[0].type).toBe('scatter');
    expect(option.series[0].data).toEqual([
      [500, 48],
      [525, 69],
    ]);
  });

  it('draws a line series for the line mark', () => {
    const option = plain(chartOption(chart({ mark: 'line' }), palette));
    expect(option.series[0].type).toBe('line');
    expect(option.tooltip.trigger).toBe('axis');
  });

  it('switches scatter plots with many points to large mode', () => {
    const points: [number, number][] = Array.from({ length: 2001 }, (_, index) => [index, index]);
    const option = plain(chartOption(chart({ points, total_points: 2001 }), palette));
    expect(option.series[0].large).toBe(true);
  });

  it('does not force the axes to include zero', () => {
    const option = plain(chartOption(chart(), palette));
    expect(option.xAxis.scale).toBe(true);
    expect(option.yAxis.scale).toBe(true);
  });

  it('takes its colours from the palette', () => {
    const option = plain(chartOption(chart(), palette));
    expect(option.color).toEqual(['#0b7577']);
    expect(option.xAxis.axisLabel.color).toBe('#666666');
  });
});

describe('chartDescription', () => {
  it('names the kind of chart, the columns and the number of points', () => {
    expect(chartDescription(chart({ total_points: 38 }))).toBe(
      'Scatter plot of counts against energy_keV, 38 points.',
    );
  });
});
