<script lang="ts">
  import type { ComponentSpec } from '../generated/ComponentSpec';
  import { registry } from '../registry';

  let { spec }: { spec: ComponentSpec } = $props();

  const Renderer = $derived(registry[spec.kind.type]);
</script>

<section class="card" aria-label={spec.title ?? undefined} data-component={spec.id}>
  {#if spec.title}
    <h2>{spec.title}</h2>
  {/if}
  <Renderer {spec} />
</section>

<style>
  .card {
    height: 100%;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-4);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
  }

  h2 {
    font-size: 0.8125rem;
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--color-text-muted);
  }
</style>
