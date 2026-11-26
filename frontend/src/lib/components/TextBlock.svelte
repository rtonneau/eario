<script lang="ts">
  import type { ComponentSpec } from '../generated/ComponentSpec';

  let { spec }: { spec: ComponentSpec } = $props();

  const paragraphs = $derived(
    spec.kind.type === 'text'
      ? spec.kind.text
          .split(/\n\s*\n/)
          .map((paragraph) => paragraph.trim())
          .filter((paragraph) => paragraph !== '')
      : [],
  );
</script>

<div class="text">
  {#each paragraphs as paragraph, index (index)}
    <p>{paragraph}</p>
  {/each}
</div>

<style>
  .text {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    max-width: 70ch;
  }
</style>
