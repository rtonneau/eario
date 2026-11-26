<script lang="ts">
  import { errorMessage } from '../api';
  import type { ComponentSpec } from '../generated/ComponentSpec';
  import type { Filter } from '../generated/Filter';
  import type { FilterOp } from '../generated/FilterOp';
  import { componentData, session } from '../session.svelte';

  let { spec }: { spec: ComponentSpec } = $props();

  const NUMERIC_OPS: { value: FilterOp; label: string }[] = [
    { value: 'gt', label: '>' },
    { value: 'ge', label: '≥' },
    { value: 'lt', label: '<' },
    { value: 'le', label: '≤' },
    { value: 'eq', label: '=' },
    { value: 'ne', label: '≠' },
  ];
  const TEXT_OPS: { value: FilterOp; label: string }[] = [
    { value: 'eq', label: 'is' },
    { value: 'ne', label: 'is not' },
    { value: 'contains', label: 'contains' },
  ];

  const kind = $derived(spec.kind.type === 'filter_input' ? spec.kind : null);
  const handle = componentData(() => spec.id);
  // Only numeric and text columns can be filtered.
  const columns = $derived(
    handle.data?.type === 'filter_input'
      ? handle.data.columns.filter((column) => column.kind === 'numeric' || column.kind === 'text')
      : [],
  );
  const active = $derived.by(() => {
    const current = kind ? session.state[kind.state] : null;
    return current !== null && typeof current === 'object' ? (current as Filter) : null;
  });

  let column = $state('');
  let op = $state<FilterOp>('gt');
  let value = $state('');
  let error = $state<string | null>(null);

  const selected = $derived(columns.find((candidate) => candidate.name === column) ?? columns[0]);
  const numeric = $derived(selected?.kind === 'numeric');
  const ops = $derived(numeric ? NUMERIC_OPS : TEXT_OPS);

  // Show the active filter in the form, for example after a page reload.
  $effect(() => {
    if (active !== null) {
      column = active.column;
      op = active.op;
      value = String(active.value);
    }
  });

  // Keep the operator valid when the column changes kind.
  $effect(() => {
    if (!ops.some((candidate) => candidate.value === op)) {
      op = ops[0]?.value ?? 'eq';
    }
  });

  function send(filter: Filter | null) {
    if (!kind) {
      return;
    }
    error = null;
    session.setState(kind.state, filter).catch((failure: unknown) => {
      error = errorMessage(failure);
    });
  }

  function apply(event: SubmitEvent) {
    event.preventDefault();
    if (!selected) {
      return;
    }
    if (numeric) {
      const number = Number(value);
      if (value.trim() === '' || !Number.isFinite(number)) {
        error = 'Enter a number.';
        return;
      }
      send({ column: selected.name, op, value: number });
    } else {
      send({ column: selected.name, op, value });
    }
  }

  function describe(filter: Filter): string {
    const label = [...NUMERIC_OPS, ...TEXT_OPS].find((candidate) => candidate.value === filter.op);
    return `${filter.column} ${label?.label ?? filter.op} ${filter.value}`;
  }
</script>

<form onsubmit={apply}>
  <div class="field">
    <label for={`filter-column-${spec.id}`}>Column</label>
    <select
      id={`filter-column-${spec.id}`}
      value={selected?.name ?? ''}
      disabled={columns.length === 0}
      onchange={(event) => (column = event.currentTarget.value)}
    >
      {#each columns as candidate (candidate.name)}
        <option value={candidate.name}>{candidate.name}</option>
      {/each}
    </select>
  </div>
  <div class="field">
    <label for={`filter-op-${spec.id}`}>Condition</label>
    <select id={`filter-op-${spec.id}`} bind:value={op} disabled={columns.length === 0}>
      {#each ops as candidate (candidate.value)}
        <option value={candidate.value}>{candidate.label}</option>
      {/each}
    </select>
  </div>
  <div class="field value">
    <label for={`filter-value-${spec.id}`}>Value</label>
    <input
      id={`filter-value-${spec.id}`}
      type="text"
      inputmode={numeric ? 'decimal' : 'text'}
      bind:value
      disabled={columns.length === 0}
    />
  </div>
  <button class="primary" type="submit" disabled={columns.length === 0}>Apply</button>
  <button type="button" disabled={active === null} onclick={() => send(null)}>Clear</button>
</form>

{#if error !== null}
  <p class="error" role="alert">{error}</p>
{:else if active !== null}
  <p role="status">Showing rows where <strong>{describe(active)}</strong></p>
{:else if handle.error !== null}
  <p class="error" role="alert">{handle.error}</p>
{:else if columns.length === 0}
  <p class="muted">No dataset loaded yet.</p>
{:else}
  <p class="muted">No filter: all rows are shown.</p>
{/if}

<style>
  form {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--space-2);
  }

  .value {
    flex: 1;
    min-width: 6rem;
  }

  .value input {
    width: 100%;
  }
</style>
