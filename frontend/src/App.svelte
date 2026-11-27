<script lang="ts">
  import { onMount } from 'svelte';

  import Header from './lib/components/Header.svelte';
  import HubView from './lib/components/HubView.svelte';
  import Notifications from './lib/components/Notifications.svelte';
  import PageView from './lib/components/PageView.svelte';
  import { session } from './lib/session.svelte';

  let pageId = $state<string | null>(null);

  const pages = $derived(session.info?.spec.pages ?? []);
  const currentPage = $derived(pages.find((page) => page.id === pageId) ?? pages[0]);

  onMount(() => {
    void session.load();
  });

  // The theme is a root attribute so the design tokens can react to it.
  $effect(() => {
    if (session.theme === 'system') {
      delete document.documentElement.dataset.theme;
    } else {
      document.documentElement.dataset.theme = session.theme;
    }
  });
</script>

{#if session.stopped}
  <main class="message">
    {#if session.hub !== null}
      <h1>The hub has stopped</h1>
      <p>
        You can close this tab. The dashboards that were running keep running. Start the hub again
        to manage them.
      </p>
    {:else}
      <h1>The application has stopped</h1>
      <p>You can close this tab. Start the application again to continue working.</p>
    {/if}
  </main>
{:else if session.fatal !== null}
  <main class="message">
    <h1>The dashboard cannot be shown</h1>
    <p role="alert">{session.fatal}</p>
  </main>
{:else if session.hub !== null}
  <HubView />
{:else if session.info === null}
  <main class="message">
    <p class="muted">Loading…</p>
  </main>
{:else}
  <Header
    title={session.info.spec.title}
    {pages}
    currentPageId={currentPage?.id ?? null}
    onSelectPage={(id) => (pageId = id)}
  />
  <main class="content">
    {#if currentPage}
      {#key currentPage.id}
        <PageView page={currentPage} />
      {/key}
    {/if}
  </main>
{/if}

<Notifications />

<style>
  .content {
    flex: 1;
    width: 100%;
    max-width: 1600px;
    margin: 0 auto;
    padding: var(--space-4);
  }

  .message {
    margin: 15vh auto 0;
    max-width: 34rem;
    padding: var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .message h1 {
    font-size: 1.375rem;
  }
</style>
