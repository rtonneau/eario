<script lang="ts">
  // Updates: the last check and what it could not check, "Check now", the
  // setting to check when the hub starts, and the update of the hub itself.

  import { onMount } from 'svelte';

  import { api, errorWithCode } from '../api';
  import type { HubInfo } from '../generated/HubInfo';
  import { checkProblems, checkSummary, formatCheckedAt, hubUpdatesItself, versionLabel } from '../hub';
  import { session } from '../session.svelte';

  let {
    info,
    announce,
  }: {
    info: HubInfo;
    /** Puts a sentence in the page's status region. */
    announce: (text: string) => void;
  } = $props();

  const id = $props.id();

  let checking = $state(false);
  let checkError = $state<string | null>(null);
  /** The stored setting, or null until the hub answered. */
  let checkAtStart = $state<boolean | null>(null);
  let savingSetting = $state(false);
  let settingError = $state<string | null>(null);
  let updatingHub = $state(false);
  let hubError = $state<string | null>(null);

  const lastCheck = $derived(info.last_check);
  const problems = $derived(lastCheck === null ? [] : checkProblems(lastCheck, info.dashboards));

  /** Time of the check last seen; not reactive. Undefined before the first list. */
  let seenCheck: number | null | undefined;

  // A check that ends is announced, whoever started it: "Check now", or the
  // check the hub runs when it starts, which the list shows a moment later.
  $effect(() => {
    const at = info.last_check?.checked_at ?? null;
    if (seenCheck !== undefined && at !== null && at !== seenCheck) {
      announce(checkSummary(info));
    }
    seenCheck = at;
  });

  onMount(() => {
    api
      .hubSettings()
      .then((settings) => {
        checkAtStart = settings.check_updates_at_start;
      })
      .catch((error: unknown) => {
        settingError = errorWithCode(error);
      });
  });

  async function checkNow() {
    if (checking) {
      return;
    }
    checking = true;
    checkError = null;
    announce('Checking for updates…');
    try {
      await session.checkForUpdates();
      if (session.hub !== null) {
        seenCheck = session.hub.last_check?.checked_at ?? null;
        announce(checkSummary(session.hub));
      }
    } catch (error) {
      checkError = errorWithCode(error);
    } finally {
      checking = false;
    }
  }

  async function setCheckAtStart(input: HTMLInputElement) {
    const wanted = input.checked;
    savingSetting = true;
    settingError = null;
    try {
      const settings = await api.hubSetCheckAtStart(wanted);
      checkAtStart = settings.check_updates_at_start;
      announce(
        settings.check_updates_at_start
          ? 'The hub will check for updates when it starts.'
          : 'The hub will not check for updates when it starts.',
      );
    } catch (error) {
      settingError = errorWithCode(error);
      // Back to what the hub holds.
      input.checked = checkAtStart ?? !wanted;
    } finally {
      savingSetting = false;
    }
  }

  async function updateHub(version: string) {
    if (
      updatingHub ||
      !window.confirm(
        `Update the hub to ${versionLabel(version)} and restart it? The hub downloads and ` +
          'verifies the release, replaces itself, then starts again at a new address and ' +
          'opens it in a new tab. Dashboards keep running.',
      )
    ) {
      return;
    }
    updatingHub = true;
    hubError = null;
    announce(`Updating the hub to ${versionLabel(version)}…`);
    try {
      await session.updateHub();
      announce(
        `The hub is updated to ${versionLabel(session.hubRestart?.version ?? version)}. It ` +
          'restarts at a new address and opens it in a new tab.',
      );
    } catch (error) {
      hubError = errorWithCode(error);
      announce('');
    } finally {
      updatingHub = false;
    }
  }
</script>

<section class="updates" aria-labelledby="{id}-heading">
  <h2 id="{id}-heading">Updates</h2>

  {#if info.available_version !== null}
    {@const version = info.available_version}
    <div class="hub-update">
      <p>
        Eario hub {versionLabel(version)} is available. This hub runs {versionLabel(
          info.eario_version,
        )}.
      </p>
      <button class="primary" disabled={updatingHub} onclick={() => void updateHub(version)}>
        Update the hub to {versionLabel(version)} and restart
      </button>
    </div>
  {/if}
  {#if hubError !== null}
    <p class="problem" role="alert">{hubError}</p>
  {/if}

  <div class="row">
    <p>
      {#if lastCheck === null}
        Not checked yet.
      {:else}
        Last checked {formatCheckedAt(lastCheck.checked_at)}.
      {/if}
    </p>
    <button disabled={checking} onclick={() => void checkNow()}>
      {checking ? 'Checking…' : 'Check now'}
    </button>
  </div>
  {#if checkError !== null}
    <p class="problem" role="alert">{checkError}</p>
  {/if}
  {#if problems.length > 0}
    <ul class="problems" aria-label="Could not check">
      {#each problems as problem (problem)}
        <li>{problem}</li>
      {/each}
    </ul>
  {/if}
  {#if !hubUpdatesItself(lastCheck)}
    <p class="muted">This build of the hub does not update itself.</p>
  {/if}

  <div class="setting">
    <input
      id="{id}-at-start"
      type="checkbox"
      checked={checkAtStart === true}
      disabled={checkAtStart === null || savingSetting}
      onchange={(event) => void setCheckAtStart(event.currentTarget)}
    />
    <label for="{id}-at-start">Check for updates at start</label>
  </div>
  {#if settingError !== null}
    <p class="problem" role="alert">{settingError}</p>
  {/if}
</section>

<style>
  .updates {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  h2 {
    font-size: 1.125rem;
  }

  .row,
  .hub-update {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2) var(--space-4);
  }

  .hub-update {
    padding: var(--space-3) var(--space-4);
    background: var(--color-surface);
    border: 1px solid var(--color-accent);
    border-radius: var(--radius);
  }

  .problem,
  .problems {
    padding: var(--space-3);
    background: var(--color-danger-surface);
    border-left: 4px solid var(--color-danger);
    border-radius: 6px;
  }

  .problems {
    margin: 0;
    padding-left: var(--space-6);
  }

  .setting {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .setting input {
    width: 1.125rem;
    height: 1.125rem;
    margin: 0;
    accent-color: var(--color-accent);
  }
</style>
