<script lang="ts">
  import { session } from '../session.svelte';
</script>

<div class="notifications" aria-live="polite">
  {#each session.notifications as notification (notification.id)}
    <div class="notification" class:error={notification.kind === 'error'}>
      <p>{notification.message}</p>
      <button aria-label="Dismiss" onclick={() => session.dismiss(notification.id)}>×</button>
    </div>
  {/each}
</div>

<style>
  .notifications {
    position: fixed;
    right: var(--space-4);
    bottom: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    max-width: min(26rem, calc(100vw - 2rem));
    z-index: 10;
  }

  .notification {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    padding: var(--space-3);
    background: var(--color-surface);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-left: 4px solid var(--color-accent);
    border-radius: 6px;
    box-shadow: 0 4px 12px rgb(0 0 0 / 0.18);
  }

  .notification.error {
    border-left-color: var(--color-danger);
    background: var(--color-danger-surface);
  }

  p {
    flex: 1;
  }

  button {
    min-height: 1.5rem;
    padding: 0 var(--space-2);
    border: none;
    background: transparent;
  }
</style>
