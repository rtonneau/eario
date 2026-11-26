# 0004. Apache ECharts, fed by renderer-independent chart data

Status: Accepted

## Context

Apache ECharts is the preferred chart engine: interactive, canvas-based,
handles large series, Apache-2.0, and usable offline. The question was what
the Rust side should produce: an ECharts option, a Charming chart, or
something of Eario's own.

## Decision

- The backend sends `ChartData`: the mark (scatter or line), axis labels,
  and points already reduced to what should be drawn, with counts of total
  and skipped rows. It contains nothing specific to ECharts.
- One frontend module, `chart-option.ts`, turns `ChartData` and the current
  theme into an ECharts option. No other module knows the option format.
- ECharts is imported per module (two series types, four components, the
  canvas renderer) and bundled; nothing is loaded from a CDN.
- Downsampling happens on the backend: at most `max_points` points (default
  5000), evenly spaced, keeping the first and last.
- Charming is not a dependency.

## Why not Charming as the chart specification

Charming is a typed Rust builder for ECharts options. Making it the
specification would:

- put a third-party type, shaped after one renderer, at the centre of the
  dashboard specification and of the MCP surface;
- send presentation (colours, fonts, theme) from the backend, although the
  theme is a frontend concern that changes at run time;
- tie Eario to its release rhythm: its latest crates.io release (0.6.0, June
  2025) predates ECharts 6.

Charming remains useful for what Eario does not do: rendering a figure
without a browser. It is the first candidate for server-side export.

## Why not send ECharts options from Rust as JSON

It would make the backend renderer-specific without the type safety Charming
offers, and duplicate theme handling.

## Consequences

- Each new chart type needs a variant of chart data in Rust and a branch in
  `chart-option.ts`. This is deliberate duplication of *structure*, not of
  chart definitions: the data shape is defined once, in Rust.
- Zoom, pan, tooltips and image export come from ECharts at no cost.
- Charts are not covered by automated tests in a real browser; the option
  builder is unit tested.

## Open questions

- **Downsampling quality.** Even sampling can miss narrow peaks in a line
  chart. Min/max per bucket or LTTB should replace it for lines, and density
  aggregation for large scatter plots.
- **Chart specification growth.** Axis scales (log), units, multiple series,
  error bars, annotations, linked selection. `ComponentKind::Chart` will need
  a richer, still renderer-independent, description. If that description
  grows enough, it moves to an `eario-charts` crate.
- **Publication-quality export** (vector output, fixed sizes).
- **Accessibility of charts** beyond the text alternative provided now.
