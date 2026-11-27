// Client-side application state.
//
// The backend owns the state of the dashboard. This module mirrors it,
// forwards changes to the backend, and tells components when their data is
// stale. It holds no logic specific to a dashboard.

import { ApiFailure, api, errorMessage, initToken } from './api';
import type { AppInfo } from './generated/AppInfo';
import type { ComponentData } from './generated/ComponentData';
import type { HubInfo } from './generated/HubInfo';
import type { StateUpdate } from './generated/StateUpdate';
import type { Theme } from './generated/Theme';
import type { JsonValue } from './generated/serde_json/JsonValue';

export interface Notification {
  id: number;
  kind: 'info' | 'error';
  message: string;
}

/** Time an informational notification stays on screen, in milliseconds. */
const INFO_NOTIFICATION_MS = 5000;

class Session {
  /** The dashboard, once loaded. */
  info = $state<AppInfo | null>(null);
  /** The hub's answer, when the backend is a hub rather than a dashboard. */
  hub = $state<HubInfo | null>(null);
  /** Mirror of the application state held by the backend. */
  state = $state<Record<string, JsonValue>>({});
  /** Per component: a counter that changes when its data is stale. */
  versions = $state<Record<string, number>>({});
  notifications = $state<Notification[]>([]);
  theme = $state<Theme>('system');
  /** Set when the dashboard cannot be shown at all. */
  fatal = $state<string | null>(null);
  /** Set once the application has been stopped from this page. */
  stopped = $state(false);

  private nextNotificationId = 1;
  /** Counts the hub actions that came back, so a list asked for earlier is dropped. */
  private hubActions = 0;

  /** Loads the dashboard from the backend. */
  async load(): Promise<void> {
    this.info = null;
    this.hub = null;
    this.fatal = null;
    if (!initToken()) {
      this.fatal =
        'This page was opened without its session token. ' +
        'Use the address printed in the terminal when the application started.';
      return;
    }
    try {
      const response = await api.app();
      if (response.kind === 'hub') {
        this.hub = response;
        document.title = 'Eario hub';
        return;
      }
      const info: AppInfo = response;
      this.info = info;
      this.state = info.state;
      this.theme = info.preferences.theme;
      document.title = info.spec.title;
    } catch (error) {
      this.fatal =
        error instanceof ApiFailure && error.status === 401
          ? 'The session token is no longer valid. Use the address printed in the terminal ' +
            'when the application started.'
          : errorMessage(error);
    }
  }

  /** Takes over the state returned by the backend and marks stale components. */
  private apply(update: StateUpdate): void {
    this.state = update.state;
    for (const id of update.invalidated) {
      this.versions[id] = (this.versions[id] ?? 0) + 1;
    }
  }

  /** Sets one state entry. Throws when the backend rejects the value. */
  async setState(key: string, value: JsonValue): Promise<void> {
    this.apply(await api.setState(key, value));
  }

  /** Imports a file into a dataset slot. Throws when the import fails. */
  async upload(dataset: string, file: File): Promise<void> {
    this.apply(await api.upload(dataset, file));
    this.notify('info', `${file.name} imported.`);
  }

  /** Loads another worksheet of an imported workbook. */
  async selectSheet(dataset: string, sheet: string): Promise<void> {
    this.apply(await api.selectSheet(dataset, sheet));
  }

  async setTheme(theme: Theme): Promise<void> {
    this.theme = theme;
    try {
      await api.setPreferences({ theme });
    } catch (error) {
      this.notify('error', `The theme could not be saved: ${errorMessage(error)}`);
    }
  }

  /**
   * Asks the hub for its list again. Throws when the hub does not answer.
   * A list that left before an action came back is dropped: it may show the
   * state from before the action.
   */
  async refreshHub(): Promise<void> {
    const actions = this.hubActions;
    const response = await api.app();
    if (response.kind !== 'hub') {
      throw new Error('This address no longer serves the hub.');
    }
    if (actions === this.hubActions) {
      this.hub = response;
    }
  }

  /** Takes over the list a hub action answers with. */
  private async hubAction(call: () => Promise<HubInfo>): Promise<void> {
    const info = await call();
    this.hubActions++;
    this.hub = info;
  }

  /** Starts an installed dashboard. Throws when the hub refuses. */
  startDashboard(slug: string): Promise<void> {
    return this.hubAction(() => api.hubStart(slug));
  }

  /** Stops a dashboard. Throws when the hub refuses. */
  stopDashboard(slug: string): Promise<void> {
    return this.hubAction(() => api.hubStop(slug));
  }

  /** Removes a dashboard from the hub. Throws when the hub refuses. */
  uninstallDashboard(slug: string): Promise<void> {
    return this.hubAction(() => api.hubUninstall(slug));
  }

  /** Stops every dashboard of the hub. Throws when the hub refuses. */
  stopAllDashboards(): Promise<void> {
    return this.hubAction(() => api.hubStopAll());
  }

  /**
   * Opens a running dashboard in a new tab, at the address the hub returns.
   * Throws when the hub refuses.
   */
  async openDashboard(slug: string): Promise<void> {
    const { url } = await api.hubOpen(slug);
    window.open(url, '_blank', 'noopener');
  }

  /** Stops the application. */
  async quit(): Promise<void> {
    try {
      await api.shutdown();
      this.stopped = true;
    } catch (error) {
      this.notify('error', errorMessage(error));
    }
  }

  notify(kind: Notification['kind'], message: string): void {
    const id = this.nextNotificationId++;
    this.notifications.push({ id, kind, message });
    if (kind === 'info') {
      setTimeout(() => this.dismiss(id), INFO_NOTIFICATION_MS);
    }
  }

  dismiss(id: number): void {
    this.notifications = this.notifications.filter((notification) => notification.id !== id);
  }
}

export const session = new Session();

/** The data of one component, kept up to date. */
export interface ComponentDataHandle {
  /** Latest data, or null before the first answer. */
  readonly data: ComponentData | null;
  /** Message of the latest failure, or null. */
  readonly error: string | null;
  readonly loading: boolean;
}

/**
 * Fetches the data of a component, and again whenever the backend reports it
 * stale or `offset` changes. Call during component initialisation.
 */
export function componentData(id: () => string, offset: () => number = () => 0): ComponentDataHandle {
  let data = $state<ComponentData | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);
  let latestRequest = 0;

  $effect(() => {
    const componentId = id();
    const requestedOffset = offset();
    // Reading the version makes the effect run again when it changes.
    void session.versions[componentId];

    const request = ++latestRequest;
    loading = true;
    api
      .componentData(componentId, requestedOffset)
      .then((result) => {
        if (request === latestRequest) {
          data = result;
          error = null;
        }
      })
      .catch((failure: unknown) => {
        if (request === latestRequest) {
          error = errorMessage(failure);
        }
      })
      .finally(() => {
        if (request === latestRequest) {
          loading = false;
        }
      });
  });

  return {
    get data() {
      return data;
    },
    get error() {
      return error;
    },
    get loading() {
      return loading;
    },
  };
}
