<script module lang="ts">
  /**
   * Time the Install or Update button stays disabled once it takes the place
   * of "Trust this key", so the second click of a double-click cannot press it.
   */
  export const CONFIRM_DELAY_MS = 600;
</script>

<script lang="ts">
  // The confirmation of an install or an update from GitHub: what the hub
  // downloaded and verified, the key that signed it and what the hub knows of
  // that key. A release whose key is not trusted yet shows a "Trust this key"
  // step first; only a full preview, with the manifest the executable
  // printed, offers to install.
  //
  // A modal <dialog>: the rest of the page is inert while it is open, Escape
  // cancels (unless a request is in flight), and focus returns to the
  // control that opened it, or to the page when that control is gone.

  import { onMount } from 'svelte';

  import { formatBytes } from '../format';
  import type { InstallPreview } from '../generated/InstallPreview';
  import { keyExplanation } from '../hub';

  let {
    preview,
    purpose,
    busy,
    error,
    opener,
    onTrust,
    onConfirm,
    onCancel,
  }: {
    preview: InstallPreview;
    /** An install from a catalog or a repository, or an update of an installed dashboard. */
    purpose: 'install' | 'update';
    /** Whether a request of this dialog is in flight. */
    busy: boolean;
    /** The refusal of the latest request, with its code, or null. */
    error: string | null;
    /** Where focus returns when the dialog closes. */
    opener: Element | null;
    onTrust: () => void;
    onConfirm: () => void;
    onCancel: () => void;
  } = $props();

  const id = $props.id();
  let dialog: HTMLDialogElement;
  let cancelButton: HTMLButtonElement;
  /** Set once the dialog is going away, so its own close is no cancel. */
  let leaving = false;

  const key = $derived(keyExplanation(preview.key_status));
  const manifest = $derived(preview.manifest);
  const subject = $derived(manifest?.name ?? preview.repo);
  const title = $derived(purpose === 'install' ? `Install ${subject}?` : `Update ${subject}?`);
  const confirmLabel = $derived(purpose === 'install' ? 'Install' : 'Update');
  const dashboardText = $derived(
    manifest === null
      ? 'Read from the executable once you trust its key'
      : `${manifest.name}, ${manifest.version === null ? 'version unknown' : `version ${manifest.version}`}`,
  );
  const progress = $derived.by(() => {
    if (busy) {
      if (manifest === null) {
        return 'Trusting the key and reading the dashboard…';
      }
      return purpose === 'install' ? 'Installing…' : 'Updating…';
    }
    return preview.key_status === 'accepted' ? `Key trusted. ${dashboardText}.` : '';
  });
  const integer = new Intl.NumberFormat('en-US');

  /** Whether the preview had its manifest when last seen; not reactive. */
  let hadManifest: boolean | undefined;
  /** Set for a moment after the install button appeared in place of Trust. */
  let settling = $state(false);
  let settleTimer: ReturnType<typeof setTimeout> | undefined;

  // Once the key is trusted, "Trust this key" gives way to the install
  // button; focus goes to Cancel rather than being lost, or landing on a
  // button that installs. That button waits a moment before it can be
  // pressed: a double-click on Trust would otherwise install.
  $effect(() => {
    const has = manifest !== null;
    if (hadManifest === false && has) {
      cancelButton.focus();
      settling = true;
      clearTimeout(settleTimer);
      settleTimer = setTimeout(() => (settling = false), CONFIRM_DELAY_MS);
    }
    hadManifest = has;
  });

  onMount(() => {
    dialog.showModal();
    cancelButton.focus();
    return () => {
      clearTimeout(settleTimer);
      leaving = true;
      if (dialog.open) {
        dialog.close();
      }
      // Back to the control that opened the dialog, or to the page when it
      // is gone or disabled (an installed entry has no Install button).
      if (
        opener instanceof HTMLElement &&
        opener !== document.body &&
        opener.isConnected &&
        !(opener as HTMLButtonElement).disabled
      ) {
        opener.focus();
      } else {
        document.querySelector<HTMLElement>('main')?.focus();
      }
    };
  });

  function cancel(event: Event) {
    // The dialog closes when this component goes, not by itself.
    event.preventDefault();
    if (!busy) {
      onCancel();
    }
  }

  function confirm() {
    // The button is disabled meanwhile; a click dispatched by script is
    // refused as well.
    if (!busy && !settling) {
      onConfirm();
    }
  }

  function closed() {
    if (leaving) {
      return;
    }
    // The browser may close a dialog without a cancel event it lets us
    // prevent, for instance on a second Escape. That is a cancel too.
    if (!busy) {
      onCancel();
    } else {
      dialog.showModal();
    }
  }
</script>

<dialog
  bind:this={dialog}
  class="install"
  aria-labelledby="{id}-title"
  aria-describedby="{id}-key-text"
  oncancel={cancel}
  onclose={closed}
>
  <h2 id="{id}-title">{title}</h2>

  <dl class="facts">
    <dt>Repository</dt>
    <dd>{preview.repo}</dd>
    <dt>Release</dt>
    <dd>{preview.tag}</dd>
    <dt>Dashboard</dt>
    <dd>{dashboardText}</dd>
    {#if manifest !== null && manifest.description !== ''}
      <dt>Description</dt>
      <dd>{manifest.description}</dd>
    {/if}
    <dt>File</dt>
    <dd class="code">{preview.asset}</dd>
    <dt>Size</dt>
    <dd>{formatBytes(preview.size)} ({integer.format(preview.size)} bytes)</dd>
    <dt>SHA-256</dt>
    <dd class="code">{preview.sha256}</dd>
    <dt>Signing key</dt>
    <dd class="code">{preview.public_key}</dd>
    <dt>Key SHA-256</dt>
    <dd class="code">{preview.key_sha256}</dd>
  </dl>

  <div
    class="key {key.level}"
    role={key.level === 'danger' ? 'alert' : undefined}
    aria-labelledby={key.level === 'danger' ? `${id}-key-title` : undefined}
  >
    <h3 id="{id}-key-title">{key.title}</h3>
    <p id="{id}-key-text">{key.text}</p>
  </div>

  <p class="visually-hidden" role="status">{progress}</p>
  {#if error !== null}
    <p class="problem" role="alert">{error}</p>
  {/if}

  <div class="buttons">
    <button bind:this={cancelButton} disabled={busy} onclick={() => onCancel()}>Cancel</button>
    {#if manifest === null}
      <button class:primary={key.level !== 'danger'} disabled={busy} onclick={() => onTrust()}>
        Trust this key
      </button>
    {:else}
      <button class="primary" disabled={busy || settling} onclick={confirm}>
        {confirmLabel}
      </button>
    {/if}
  </div>
</dialog>

<style>
  .install {
    width: min(42rem, calc(100vw - 2rem));
    max-height: calc(100vh - 2rem);
    padding: var(--space-6);
    color: var(--color-text);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
  }

  .install::backdrop {
    background: var(--color-backdrop);
  }

  h2 {
    font-size: 1.125rem;
    margin-bottom: var(--space-4);
  }

  h3 {
    font-size: 1rem;
  }

  .facts {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: var(--space-1) var(--space-4);
    margin: 0 0 var(--space-4);
  }

  dt {
    font-weight: 600;
    color: var(--color-text-muted);
  }

  dd {
    margin: 0;
    min-width: 0;
  }

  .code {
    font-family: var(--font-mono);
    font-size: 0.8125rem;
    overflow-wrap: anywhere;
  }

  .key {
    padding: var(--space-3);
    border-left: 4px solid var(--color-accent);
    border-radius: 6px;
    background: var(--color-surface-raised);
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .key.warning {
    background: var(--color-warning-surface);
    border-left-color: var(--color-warning);
  }

  .key.danger {
    background: var(--color-danger-surface);
    border: 2px solid var(--color-danger);
    border-left-width: 8px;
  }

  .problem {
    margin-top: var(--space-3);
    padding: var(--space-3);
    background: var(--color-danger-surface);
    border-left: 4px solid var(--color-danger);
    border-radius: 6px;
  }

  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-4);
  }
</style>
