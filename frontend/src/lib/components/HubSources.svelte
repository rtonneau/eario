<script lang="ts">
  // Where dashboards come from on GitHub: a repository the user names, and
  // the catalogs the hub reads. Installing opens the confirmation dialog of
  // the hub's page; this component only lists and asks.
  //
  // The hub reads every catalog from GitHub on each request, so the list is
  // loaded once, and again only when the user changes it or asks.

  import { onMount } from 'svelte';

  import { ApiFailure, api, errorWithCode } from '../api';
  import type { HubCatalog } from '../generated/HubCatalog';
  import type { HubCatalogEntry } from '../generated/HubCatalogEntry';
  import type { HubDashboard } from '../generated/HubDashboard';
  import { installedFromEntry, looksLikeRepo, looksLikeSlug } from '../hub';

  let {
    dashboards,
    previewing,
    installing,
    onInstall,
    announce,
  }: {
    dashboards: HubDashboard[];
    /**
     * Whether a preview, for an install or an update, is in flight: one runs
     * at a time, so every control that asks for one waits.
     */
    previewing: boolean;
    /** The repository (and slug) whose release is being previewed for an install, or null. */
    installing: { repo: string; slug: string | null } | null;
    /**
     * Previews the latest release of a repository: of its dashboard `slug`,
     * or, with null, of its only one. Rejects when the hub refuses.
     */
    onInstall: (repo: string, slug: string | null) => Promise<void>;
    /** Puts a sentence in the page's status region. */
    announce: (text: string) => void;
  } = $props();

  const id = $props.id();

  /** The hub's catalogs, or null before the first answer. */
  let catalogs = $state<HubCatalog[] | null>(null);
  /** Whether a catalog request is in flight. */
  let loading = $state(false);
  /** The refusal of the latest catalog request. */
  let catalogError = $state<string | null>(null);
  /** The refusal of the latest install request started here. */
  let installError = $state<string | null>(null);

  let repository = $state('');
  let repositoryInvalid = $state(false);
  let slug = $state('');
  let slugInvalid = $state(false);
  let slugInput = $state<HTMLInputElement | null>(null);
  let catalogRepository = $state('');
  let catalogInvalid = $state(false);

  async function catalogRequest(work: () => Promise<HubCatalog[]>, done?: string) {
    if (loading) {
      return;
    }
    loading = true;
    catalogError = null;
    try {
      catalogs = await work();
      if (done !== undefined) {
        announce(done);
      }
    } catch (error) {
      catalogError = errorWithCode(error);
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    void catalogRequest(() => api.hubCatalogs());
  });

  async function install(repo: string, dashboard: string | null) {
    installError = null;
    try {
      await onInstall(repo, dashboard);
    } catch (error) {
      installError = errorWithCode(error);
      // The repository publishes several dashboards: the message names
      // them, and the slug field is where the user picks one.
      if (error instanceof ApiFailure && error.code === 'hub_ambiguous_asset') {
        // From a catalog entry too: the form must name that repository, or
        // the slug typed next would go with another one.
        repository = repo;
        repositoryInvalid = false;
        slugInput?.focus();
      }
    }
  }

  function previewRepository(event: SubmitEvent) {
    event.preventDefault();
    // A new attempt: the refusal of the last one no longer applies.
    installError = null;
    const repo = repository.trim();
    const dashboard = slug.trim();
    repositoryInvalid = !looksLikeRepo(repo);
    slugInvalid = dashboard !== '' && !looksLikeSlug(dashboard);
    if (!repositoryInvalid && !slugInvalid) {
      void install(repo, dashboard === '' ? null : dashboard);
    }
  }

  function isInstalling(entry: HubCatalogEntry): boolean {
    return installing?.repo === entry.repo && installing.slug === entry.slug;
  }

  function addCatalog(event: SubmitEvent) {
    event.preventDefault();
    const repo = catalogRepository.trim();
    catalogInvalid = !looksLikeRepo(repo);
    if (catalogInvalid) {
      return;
    }
    void catalogRequest(async () => {
      const listing = await api.hubAddCatalog(repo);
      catalogRepository = '';
      return listing;
    }, `Catalog ${repo} added.`);
  }

  function removeCatalog(catalog: HubCatalog) {
    if (!window.confirm(`Remove the catalog ${catalog.repo}? Installed dashboards stay installed.`)) {
      return;
    }
    void catalogRequest(() => api.hubRemoveCatalog(catalog.repo), `Catalog ${catalog.repo} removed.`);
  }
</script>

<section class="sources" aria-labelledby="{id}-heading">
  <h2 id="{id}-heading">Install from GitHub</h2>

  <form class="field" onsubmit={previewRepository} novalidate>
    <div class="row fields">
      <div class="field repository">
        <label for="{id}-repository">Repository (owner/repo)</label>
        <input
          id="{id}-repository"
          type="text"
          autocomplete="off"
          spellcheck="false"
          bind:value={repository}
          aria-invalid={repositoryInvalid}
          aria-describedby={repositoryInvalid ? `${id}-repository-hint` : undefined}
          oninput={() => (repositoryInvalid = false)}
        />
      </div>
      <div class="field slug">
        <label for="{id}-slug">Dashboard slug (optional)</label>
        <input
          id="{id}-slug"
          type="text"
          autocomplete="off"
          spellcheck="false"
          bind:this={slugInput}
          bind:value={slug}
          aria-invalid={slugInvalid}
          aria-describedby={slugInvalid ? `${id}-slug-help ${id}-slug-hint` : `${id}-slug-help`}
          oninput={() => (slugInvalid = false)}
        />
      </div>
      <button type="submit" disabled={previewing}>Add repository</button>
    </div>
    <p id="{id}-slug-help" class="muted">
      Give the slug only when the repository publishes several dashboards, for example
      signal-lab.
    </p>
    {#if repositoryInvalid}
      <p id="{id}-repository-hint" class="hint" role="alert">
        Enter a repository as owner/repo, for example lab/signal-lab.
      </p>
    {/if}
    {#if slugInvalid}
      <p id="{id}-slug-hint" class="hint" role="alert">
        Enter a slug of lower-case letters, digits and single dashes, for example signal-lab.
      </p>
    {/if}
  </form>
  {#if installError !== null}
    <p class="problem" role="alert">{installError}</p>
  {/if}

  <div class="toolbar">
    <h3>Catalogs</h3>
    <button
      disabled={loading}
      onclick={() => void catalogRequest(() => api.hubCatalogs(), 'Catalogs reloaded.')}
    >
      Reload catalogs
    </button>
  </div>
  {#if catalogError !== null}
    <p class="problem" role="alert">{catalogError}</p>
  {/if}
  {#if catalogs === null}
    <p class="muted">{loading ? 'Reading the catalogs…' : 'The catalogs could not be read.'}</p>
  {:else if catalogs.length === 0}
    <p class="muted">No catalog. Add one below.</p>
  {:else}
    <ul class="catalogs" aria-label="Catalogs">
      <!-- Keyed by position too: a list that names a repository twice must
           not break the page. -->
      {#each catalogs as catalog, index (`${index}:${catalog.repo}`)}
        <li class="catalog">
          <div class="catalog-head">
            <div>
              <h4>{catalog.name ?? catalog.repo}</h4>
              <p class="muted">{catalog.repo}</p>
            </div>
            <button
              aria-label="Remove catalog {catalog.repo}"
              disabled={loading}
              onclick={() => removeCatalog(catalog)}
            >
              Remove
            </button>
          </div>
          {#if catalog.error !== null}
            <p class="problem">
              This catalog could not be read: {catalog.error.message} ({catalog.error.code})
            </p>
          {:else if catalog.dashboards.length === 0}
            <p class="muted">This catalog lists no dashboard.</p>
          {:else}
            <ul class="entries" aria-label="Dashboards of {catalog.name ?? catalog.repo}">
              <!-- A repository may be listed once per slug. -->
              {#each catalog.dashboards as entry, index (`${index}:${entry.repo}:${entry.slug ?? ''}`)}
                {@const installed = installedFromEntry(entry, dashboards)}
                <li class="entry">
                  <div class="identity">
                    <p class="name">{entry.name}</p>
                    <p class="muted">
                      {entry.repo}{#if entry.slug !== null}, dashboard {entry.slug}{/if}
                    </p>
                    {#if entry.description !== ''}
                      <p class="muted">{entry.description}</p>
                    {/if}
                  </div>
                  {#if installed !== undefined}
                    <p class="installed">Installed</p>
                  {:else}
                    <button
                      class="primary"
                      aria-label="Install {entry.name} from {entry.repo}"
                      disabled={previewing}
                      onclick={() => void install(entry.repo, entry.slug)}
                    >
                      {isInstalling(entry) ? 'Verifying…' : 'Install'}
                    </button>
                  {/if}
                </li>
              {/each}
            </ul>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}

  <form class="field" onsubmit={addCatalog} novalidate>
    <label for="{id}-catalog">Catalog repository (owner/repo)</label>
    <div class="row">
      <input
        id="{id}-catalog"
        type="text"
        autocomplete="off"
        spellcheck="false"
        bind:value={catalogRepository}
        aria-invalid={catalogInvalid}
        aria-describedby={catalogInvalid ? `${id}-catalog-hint` : undefined}
        oninput={() => (catalogInvalid = false)}
      />
      <button type="submit" disabled={loading}>Add catalog</button>
    </div>
    {#if catalogInvalid}
      <p id="{id}-catalog-hint" class="hint" role="alert">
        Enter a repository as owner/repo, for example lab/catalog.
      </p>
    {/if}
  </form>
</section>

<style>
  .sources {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  h2 {
    font-size: 1.125rem;
  }

  h3,
  h4 {
    font-size: 1rem;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  label {
    font-weight: 600;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .row > input {
    flex: 1 1 16rem;
    min-width: 0;
    max-width: 28rem;
  }

  .fields {
    align-items: flex-end;
  }

  .fields .repository {
    flex: 1 1 16rem;
    min-width: 0;
    max-width: 28rem;
  }

  .fields .slug {
    flex: 1 1 12rem;
    min-width: 0;
    max-width: 16rem;
  }

  .hint {
    color: var(--color-danger);
  }

  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    margin-top: var(--space-2);
  }

  .problem {
    padding: var(--space-3);
    background: var(--color-danger-surface);
    border-left: 4px solid var(--color-danger);
    border-radius: 6px;
  }

  .catalogs,
  .entries {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .catalog {
    padding: var(--space-3) var(--space-4);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .catalog-head,
  .entry {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2) var(--space-4);
  }

  .entry {
    padding-top: var(--space-2);
    border-top: 1px solid var(--color-border);
  }

  .identity {
    flex: 1 1 12rem;
    min-width: 0;
  }

  .name {
    font-weight: 600;
  }

  .installed {
    font-weight: 600;
    color: var(--color-text-muted);
  }
</style>
