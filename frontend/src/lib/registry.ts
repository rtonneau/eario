// Maps each kind of component of the dashboard specification to the Svelte
// component that renders it. Adding a kind to the Rust `ComponentKind` enum
// makes this file fail to type-check until a renderer is registered.

import type { Component } from 'svelte';

import Chart from './components/Chart.svelte';
import ColumnSelect from './components/ColumnSelect.svelte';
import DataTable from './components/DataTable.svelte';
import DatasetInfo from './components/DatasetInfo.svelte';
import FileImport from './components/FileImport.svelte';
import FilterInput from './components/FilterInput.svelte';
import Statistics from './components/Statistics.svelte';
import TextBlock from './components/TextBlock.svelte';
import type { ComponentKind } from './generated/ComponentKind';
import type { ComponentSpec } from './generated/ComponentSpec';

export type ComponentRenderer = Component<{ spec: ComponentSpec }>;

export const registry: Record<ComponentKind['type'], ComponentRenderer> = {
  text: TextBlock,
  file_import: FileImport,
  dataset_info: DatasetInfo,
  column_select: ColumnSelect,
  filter_input: FilterInput,
  data_table: DataTable,
  chart: Chart,
  statistics: Statistics,
};
