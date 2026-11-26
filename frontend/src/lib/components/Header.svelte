<script lang="ts">
  import type { PageSpec } from '../generated/PageSpec';
  import type { Theme } from '../generated/Theme';
  import { session } from '../session.svelte';
  import Logo from './Logo.svelte';

  let {
    title,
    pages,
    currentPageId,
    onSelectPage,
  }: {
    title: string;
    pages: PageSpec[];
    currentPageId: string | null;
    onSelectPage: (id: string) => void;
  } = $props();

  const themes: { value: Theme; label: string }[] = [
    { value: 'system', label: 'System theme' },
    { value: 'light', label: 'Light theme' },
    { value: 'dark', label: 'Dark theme' },
  ];

  function quit() {
    if (window.confirm('Stop the application? The data loaded in this session will be closed.')) {
      void session.quit();
    }
  }
</script>

<header>
  <div class="brand">
    <Logo />
    <h1>{title}</h1>
  </div>

  {#if pages.length > 1}
    <nav aria-label="Pages">
      {#each pages as page (page.id)}
        <button
          class="tab"
          aria-current={page.id === currentPageId ? 'page' : undefined}
          onclick={() => onSelectPage(page.id)}
        >
          {page.title}
        </button>
      {/each}
    </nav>
  {/if}

  <div class="actions">
    <label class="visually-hidden" for="theme">Theme</label>
    <select
      id="theme"
      value={session.theme}
      onchange={(event) => void session.setTheme(event.currentTarget.value as Theme)}
    >
      {#each themes as theme (theme.value)}
        <option value={theme.value}>{theme.label}</option>
      {/each}
    </select>
    <button onclick={quit}>Quit</button>
  </div>
</header>

<style>
  header {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-4);
    padding: var(--space-2) var(--space-4);
    background: var(--color-header);
    color: var(--color-header-text);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  h1 {
    font-size: 1.0625rem;
    font-weight: 600;
  }

  nav {
    display: flex;
    gap: var(--space-1);
    flex: 1;
  }

  .actions {
    display: flex;
    gap: var(--space-2);
    margin-left: auto;
  }

  /* Controls in the header sit on the brand colour in both themes. */
  header button,
  header select {
    background: transparent;
    color: var(--color-header-text);
    border-color: rgb(230 237 245 / 0.35);
  }

  header select option {
    color: var(--color-text);
    background: var(--color-surface);
  }

  header button:hover:not(:disabled) {
    background: rgb(230 237 245 / 0.12);
  }

  header :global(:focus-visible) {
    outline-color: var(--color-star);
  }

  .tab {
    border-color: transparent;
  }

  .tab[aria-current='page'] {
    background: rgb(230 237 245 / 0.16);
    font-weight: 600;
  }
</style>
