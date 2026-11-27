<script module lang="ts">
  /** Time between two requests for the hub's list while the page is visible. */
  export const POLL_INTERVAL_MS = 2000;
</script>

<script lang="ts">
  import { onMount } from 'svelte';

  import { errorMessage } from '../api';
  import type { DashboardState } from '../generated/DashboardState';
  import type { HubDashboard } from '../generated/HubDashboard';
  import { session } from '../session.svelte';
  import Header from './Header.svelte';

  const stateLabels: Record<DashboardState, string> = {
    starting: 'Starting',
    running: 'Running',
    stopped: 'Stopped',
    failed: 'Failed',
  };

  /** Keys of the requests in flight: a slug, or `*` for Stop all. */
  let busy = $state<string[]>([]);
  /** The refusal of the latest action, or null. */
  let actionError = $state<string | null>(null);
  /** Set while the hub does not answer the periodic request. */
  let pollError = $state<string | null>(null);
  /** Text of the status region: what changed last. */
  let announcement = $state('');

  const dashboards = $derived(session.hub?.dashboards ?? []);
  const anyActive = $derived(
    dashboards.some((dashboard) => dashboard.state === 'running' || dashboard.state === 'starting'),
  );

  /** State of each dashboard the last time the list changed; not reactive. */
  const known = new Map<string, DashboardState>();

  // A state change is announced whether the user caused it or not: a start
  // becomes a running dashboard a moment later, a dashboard may fail or be
  // stopped from another window.
  $effect(() => {
    const changes: string[] = [];
    for (const dashboard of dashboards) {
      const before = known.get(dashboard.slug);
      if (before !== undefined && before !== dashboard.state) {
        changes.push(
          dashboard.state === 'failed'
            ? `${dashboard.name} failed to start.`
            : `${dashboard.name} is ${stateLabels[dashboard.state].toLowerCase()}.`,
        );
      }
      known.set(dashboard.slug, dashboard.state);
    }
    for (const slug of [...known.keys()]) {
      if (!dashboards.some((dashboard) => dashboard.slug === slug)) {
        known.delete(slug);
      }
    }
    if (changes.length > 0) {
      announcement = changes.join(' ');
    }
  });

  onMount(() => {
    let polling = false;
    async function poll() {
      if (polling || document.visibilityState !== 'visible') {
        return;
      }
      polling = true;
      try {
        await session.refreshHub();
        pollError = null;
      } catch (error) {
        pollError = errorMessage(error);
      } finally {
        polling = false;
      }
    }
    const timer = setInterval(() => void poll(), POLL_INTERVAL_MS);
    // A tab that comes back to the front catches up at once.
    const onVisible = () => void poll();
    document.addEventListener('visibilitychange', onVisible);
    return () => {
      clearInterval(timer);
      document.removeEventListener('visibilitychange', onVisible);
    };
  });

  function isBusy(slug: string): boolean {
    return busy.includes(slug) || busy.includes('*');
  }

  /** Runs one request, keeps its control disabled meanwhile, and reports a refusal. */
  async function perform(key: string, work: () => Promise<void>, done?: string): Promise<void> {
    if (isBusy(key)) {
      return;
    }
    busy = [...busy, key];
    actionError = null;
    try {
      await work();
      if (done !== undefined) {
        announcement = done;
      }
    } catch (error) {
      actionError = errorMessage(error);
    } finally {
      busy = busy.filter((entry) => entry !== key);
    }
  }

  function start(dashboard: HubDashboard) {
    void perform(dashboard.slug, () => session.startDashboard(dashboard.slug));
  }

  function stop(dashboard: HubDashboard) {
    void perform(dashboard.slug, () => session.stopDashboard(dashboard.slug));
  }

  function open(dashboard: HubDashboard) {
    void perform(
      dashboard.slug,
      () => session.openDashboard(dashboard.slug),
      `${dashboard.name} opened in a new tab.`,
    );
  }

  function uninstall(dashboard: HubDashboard) {
    if (
      !window.confirm(
        `Uninstall ${dashboard.name}? Its copy is removed from the hub. ` +
          'Run its executable with --eario-install to add it again.',
      )
    ) {
      return;
    }
    void perform(
      dashboard.slug,
      () => session.uninstallDashboard(dashboard.slug),
      `${dashboard.name} uninstalled.`,
    );
  }

  function stopAll() {
    void perform('*', () => session.stopAllDashboards());
  }
</script>

<Header
  title="Eario hub"
  pages={[]}
  currentPageId={null}
  onSelectPage={() => {}}
  themeChoice={false}
  quitLabel="Quit hub"
  quitQuestion="Quit the hub? The dashboards that are running keep running."
/>
<main class="content">
  <div class="toolbar">
    <h2>Installed dashboards</h2>
    <button onclick={stopAll} disabled={!anyActive || busy.includes('*')}>Stop all</button>
  </div>

  <p class="visually-hidden" role="status">{announcement}</p>
  {#if actionError !== null}
    <p class="problem" role="alert">{actionError}</p>
  {/if}
  {#if pollError !== null}
    <p class="problem" role="alert">The hub is not responding: {pollError}</p>
  {/if}

  {#if dashboards.length === 0}
    <p class="muted">
      No dashboard is installed. Run a dashboard's executable with
      <code>--eario-install</code> to add it to the hub.
    </p>
  {:else}
    <ul class="dashboards" aria-label="Installed dashboards">
      {#each dashboards as dashboard (dashboard.slug)}
        {@const working = isBusy(dashboard.slug)}
        <li class="dashboard">
          <div class="summary">
            <div class="identity">
              <h3>{dashboard.name}</h3>
              <p class="muted">
                {dashboard.version === null ? 'Version unknown' : `Version ${dashboard.version}`}
              </p>
            </div>
            <p class="state {dashboard.state}">
              <span class="dot" aria-hidden="true"></span>
              <span class="visually-hidden">State: </span><span>{stateLabels[dashboard.state]}</span>
            </p>
            <div class="actions">
              <button
                class="primary"
                aria-label="Start {dashboard.name}"
                disabled={working || (dashboard.state !== 'stopped' && dashboard.state !== 'failed')}
                onclick={() => start(dashboard)}
              >
                Start
              </button>
              <button
                aria-label="Open {dashboard.name}"
                disabled={working || dashboard.state !== 'running'}
                onclick={() => open(dashboard)}
              >
                Open
              </button>
              <button
                aria-label="Stop {dashboard.name}"
                disabled={working ||
                  (dashboard.state !== 'running' && dashboard.state !== 'starting')}
                onclick={() => stop(dashboard)}
              >
                Stop
              </button>
              <button
                aria-label="Uninstall {dashboard.name}"
                disabled={working || (dashboard.state !== 'stopped' && dashboard.state !== 'failed')}
                onclick={() => uninstall(dashboard)}
              >
                Uninstall
              </button>
            </div>
          </div>
          {#if dashboard.description !== ''}
            <p class="description">{dashboard.description}</p>
          {/if}
          {#if dashboard.state === 'failed' && dashboard.log_lines.length > 0}
            <!-- The log scrolls, so a keyboard user must be able to focus it;
                 the region names it for whoever lands on it. -->
            <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
            <pre
              class="log"
              role="region"
              aria-label="Last log lines of {dashboard.name}"
              tabindex="0">{dashboard.log_lines.join('\n')}</pre>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}

  <p class="muted footer">Eario {session.hub?.eario_version}</p>
</main>

<style>
  .content {
    flex: 1;
    width: 100%;
    max-width: 60rem;
    margin: 0 auto;
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
  }

  h2 {
    font-size: 1.125rem;
  }

  h3 {
    font-size: 1rem;
  }

  .problem {
    padding: var(--space-3);
    background: var(--color-danger-surface);
    border-left: 4px solid var(--color-danger);
    border-radius: 6px;
  }

  .dashboards {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .dashboard {
    padding: var(--space-3) var(--space-4);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .summary {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3) var(--space-4);
  }

  .identity {
    flex: 1 1 12rem;
    min-width: 0;
  }

  .state {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 6.5rem;
    font-weight: 600;
  }

  .dot {
    width: 0.625rem;
    height: 0.625rem;
    border-radius: 50%;
    background: var(--color-text-muted);
  }

  .state.running .dot {
    background: var(--color-accent);
  }

  .state.failed .dot {
    background: var(--color-danger);
  }

  .state.starting .dot {
    background: transparent;
    border: 2px solid var(--color-text-muted);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .description {
    color: var(--color-text-muted);
  }

  .log {
    margin: 0;
    padding: var(--space-3);
    max-height: 12rem;
    overflow: auto;
    background: var(--color-surface-raised);
    border: 1px solid var(--color-border);
    border-radius: 6px;
    font-family: var(--font-mono);
    font-size: 0.8125rem;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .footer {
    font-size: 0.8125rem;
  }
</style>
