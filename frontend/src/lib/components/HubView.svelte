<script module lang="ts">
  /** Time between two requests for the hub's list while the page is visible. */
  export const POLL_INTERVAL_MS = 2000;
</script>

<script lang="ts">
  import { onMount } from 'svelte';

  import { api, errorMessage, errorWithCode } from '../api';
  import type { DashboardState } from '../generated/DashboardState';
  import type { HubDashboard } from '../generated/HubDashboard';
  import type { InstallPreview } from '../generated/InstallPreview';
  import { sourceText, versionLabel } from '../hub';
  import { session } from '../session.svelte';
  import Header from './Header.svelte';
  import HubSources from './HubSources.svelte';
  import HubUpdates from './HubUpdates.svelte';
  import InstallDialog from './InstallDialog.svelte';

  const stateLabels: Record<DashboardState, string> = {
    starting: 'Starting',
    running: 'Running',
    stopped: 'Stopped',
    failed: 'Failed',
  };

  /** An install or an update the user is confirming in the dialog. */
  interface Confirmation {
    /** The dashboard updated; null for an install. */
    slug: string | null;
    preview: InstallPreview;
    busy: boolean;
    error: string | null;
    /**
     * What had focus when the user asked, before its button was disabled
     * during the preview: focus returns there when the dialog closes.
     */
    opener: Element | null;
  }

  /** Keys of the requests in flight: a slug, or `*` for Stop all. */
  let busy = $state<string[]>([]);
  /** The refusal of the latest action, or null. */
  let actionError = $state<string | null>(null);
  /** Set while the hub does not answer the periodic request. */
  let pollError = $state<string | null>(null);
  /** Text of the status region: what changed last. */
  let announcement = $state('');
  /** The install or update shown in the dialog, or null. */
  let confirmation = $state<Confirmation | null>(null);
  /**
   * Set while a preview, for an install or an update, is in flight. One runs
   * at a time, and every Install, Update and "Add repository" control waits:
   * the dialog must show the release the user asked for last, not another
   * one that answered later.
   */
  let previewing = $state(false);
  /** The repository whose release is being previewed for an install, or null. */
  let installing = $state<string | null>(null);
  /** Number of the latest preview asked for; not reactive. */
  let latestPreview = 0;

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
      // Once the hub has replaced itself it no longer answers here: its new
      // version opens its own tab, at another address.
      if (polling || document.visibilityState !== 'visible' || session.hubRestart !== null) {
        return;
      }
      polling = true;
      try {
        await session.refreshHub();
        pollError = null;
      } catch (error) {
        pollError = session.hubRestart === null ? errorMessage(error) : null;
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

  function announce(text: string) {
    announcement = text;
  }

  function isBusy(slug: string): boolean {
    return busy.includes(slug) || busy.includes('*');
  }

  /**
   * Runs one request, keeps its control disabled meanwhile, and reports a
   * refusal. `done` is announced on success; given as a function, it is
   * called once the hub answered.
   */
  async function perform(
    key: string,
    work: () => Promise<void>,
    done?: string | (() => string),
  ): Promise<void> {
    if (isBusy(key)) {
      return;
    }
    busy = [...busy, key];
    actionError = null;
    try {
      await work();
      if (done !== undefined) {
        announcement = typeof done === 'string' ? done : done();
      }
    } catch (error) {
      actionError = errorWithCode(error);
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
          (dashboard.source.kind === 'github'
            ? `Install it from ${dashboard.source.repo} again to add it back.`
            : 'Run its executable with --eario-install to add it again.'),
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

  /** The version `slug` has now, as the hub last listed it. */
  function versionNow(slug: string): string {
    const version = session.hub?.dashboards.find((entry) => entry.slug === slug)?.version;
    return version === null || version === undefined ? 'an unknown version' : `version ${version}`;
  }

  /**
   * Asks the hub for a preview, then shows it in the dialog for `slug` (null
   * for an install). Does nothing while another preview runs or the dialog is
   * open. Rejects when the hub refuses, for the caller to show where it asked.
   */
  async function preview(
    slug: string | null,
    progress: string,
    request: () => Promise<InstallPreview>,
  ): Promise<void> {
    if (previewing || confirmation !== null) {
      return;
    }
    // Read before the control that asked is disabled: a browser moves the
    // focus off a disabled button.
    const opener = document.activeElement;
    const number = ++latestPreview;
    previewing = true;
    announcement = progress;
    try {
      const answer = await request();
      // Shown only when it is the preview asked for last and no dialog is
      // open: the user never asked to confirm another one.
      if (number === latestPreview && confirmation === null) {
        confirmation = { slug, preview: answer, busy: false, error: null, opener };
      }
    } finally {
      if (number === latestPreview) {
        previewing = false;
        announcement = '';
      }
    }
  }

  /** Previews the latest release of `repo` for an install. */
  async function install(repo: string): Promise<void> {
    if (previewing || confirmation !== null) {
      return;
    }
    installing = repo;
    try {
      await preview(null, `Downloading and verifying the latest release of ${repo}…`, () =>
        api.hubPreview(repo),
      );
    } finally {
      installing = null;
    }
  }

  function update(dashboard: HubDashboard) {
    if (previewing || confirmation !== null) {
      return;
    }
    void perform(dashboard.slug, () =>
      preview(
        dashboard.slug,
        `Downloading and verifying the update of ${dashboard.name}…`,
        () => api.hubUpdatePreview(dashboard.slug),
      ),
    );
  }

  function rollback(dashboard: HubDashboard) {
    if (
      !window.confirm(
        `Roll ${dashboard.name} back to the version installed before its last update? ` +
          'The current version is removed.',
      )
    ) {
      return;
    }
    void perform(
      dashboard.slug,
      () => session.rollbackDashboard(dashboard.slug),
      () => `${dashboard.name} rolled back to ${versionNow(dashboard.slug)}.`,
    );
  }

  /** Accepts the previewed key by its SHA-256, then shows the full preview. */
  async function trustKey() {
    const current = confirmation;
    if (current === null || current.busy) {
      return;
    }
    current.busy = true;
    current.error = null;
    try {
      current.preview = await api.hubAcceptKey(current.preview.repo, current.preview.key_sha256);
    } catch (error) {
      current.error = errorWithCode(error);
    } finally {
      current.busy = false;
    }
  }

  /** Installs or updates by the previewed SHA-256. */
  async function confirmInstall() {
    const current = confirmation;
    if (current === null || current.busy || current.preview.manifest === null) {
      return;
    }
    const { preview, slug } = current;
    const name = preview.manifest?.name ?? preview.repo;
    current.busy = true;
    current.error = null;
    try {
      if (slug === null) {
        await session.installFromGitHub(preview.repo, preview.sha256);
        announcement = `${name} installed.`;
      } else {
        await session.updateDashboard(slug, preview.sha256);
        announcement = `${name} updated to ${versionNow(slug)}.`;
      }
      confirmation = null;
    } catch (error) {
      current.error = errorWithCode(error);
    } finally {
      current.busy = false;
    }
  }

  function cancelConfirmation() {
    const purpose = confirmation?.slug === null ? 'installed' : 'updated';
    confirmation = null;
    announcement = `Nothing was ${purpose}.`;
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
<!-- Focusable from script only: focus lands here when the control that
     opened the dialog is gone. -->
<main class="content" tabindex="-1">
  <p class="visually-hidden" role="status">{announcement}</p>

  {#if session.hubRestart !== null}
    <section class="restart">
      <h2>The hub is restarting</h2>
      <p>
        The hub is updated from {versionLabel(session.hubRestart.previous_version)} to
        {versionLabel(session.hubRestart.version)}. It starts again at a new address and opens it
        in a new tab. You can close this tab. The dashboards that were running keep running.
      </p>
    </section>
  {:else}
    <div class="toolbar">
      <h2>Installed dashboards</h2>
      <button onclick={stopAll} disabled={!anyActive || busy.includes('*')}>Stop all</button>
    </div>

    {#if actionError !== null}
      <p class="problem" role="alert">{actionError}</p>
    {/if}
    {#if pollError !== null}
      <p class="problem" role="alert">The hub is not responding: {pollError}</p>
    {/if}

    {#if dashboards.length === 0}
      <p class="muted">
        No dashboard is installed. Install one from GitHub below, or run a dashboard's executable
        with <code>--eario-install</code> to add it to the hub.
      </p>
    {:else}
      <ul class="dashboards" aria-label="Installed dashboards">
        {#each dashboards as dashboard (dashboard.slug)}
          {@const working = isBusy(dashboard.slug)}
          {@const idle = dashboard.state === 'stopped' || dashboard.state === 'failed'}
          {@const fromGitHub = dashboard.source.kind === 'github'}
          <li class="dashboard">
            <div class="summary">
              <div class="identity">
                <h3>{dashboard.name}</h3>
                <p class="muted">
                  {dashboard.version === null ? 'Version unknown' : `Version ${dashboard.version}`}
                </p>
                <p class="muted">{sourceText(dashboard)}</p>
              </div>
              {#if fromGitHub && dashboard.available_version !== null}
                <p class="badge">{versionLabel(dashboard.available_version)} available</p>
              {/if}
              <p class="state {dashboard.state}">
                <span class="dot" aria-hidden="true"></span>
                <span class="visually-hidden">State: </span><span>{stateLabels[dashboard.state]}</span>
              </p>
              <div class="actions">
                <button
                  class="primary"
                  aria-label="Start {dashboard.name}"
                  disabled={working || !idle}
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
                  disabled={working || !idle}
                  onclick={() => uninstall(dashboard)}
                >
                  Uninstall
                </button>
                {#if fromGitHub && dashboard.available_version !== null}
                  <button
                    aria-label="Update {dashboard.name}"
                    disabled={working || !idle || previewing}
                    onclick={() => update(dashboard)}
                  >
                    Update
                  </button>
                {/if}
                {#if fromGitHub && dashboard.has_previous}
                  <button
                    aria-label="Roll back {dashboard.name}"
                    disabled={working || !idle}
                    onclick={() => rollback(dashboard)}
                  >
                    Roll back
                  </button>
                {/if}
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

    {#if session.hub !== null}
      <HubUpdates info={session.hub} {announce} />
    {/if}

    <HubSources {dashboards} {previewing} {installing} onInstall={install} {announce} />

    <p class="muted footer">Eario {session.hub?.eario_version}</p>
  {/if}
</main>

{#if confirmation !== null}
  <InstallDialog
    preview={confirmation.preview}
    purpose={confirmation.slug === null ? 'install' : 'update'}
    busy={confirmation.busy}
    error={confirmation.error}
    opener={confirmation.opener}
    onTrust={() => void trustKey()}
    onConfirm={() => void confirmInstall()}
    onCancel={cancelConfirmation}
  />
{/if}

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

  .content:focus {
    outline: none;
  }

  .restart {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    max-width: 34rem;
    margin: 10vh auto 0;
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

  .badge {
    padding: 0 var(--space-2);
    border-radius: 999px;
    background: var(--color-accent);
    color: var(--color-accent-contrast);
    font-size: 0.8125rem;
    font-weight: 600;
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
