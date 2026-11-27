// Renders the hub's page through the whole application, with the hub replaced
// by a small in-memory model that answers the routes of its HTTP API.

import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import App from '../../App.svelte';
import type { HubDashboard } from '../generated/HubDashboard';
import type { HubInfo } from '../generated/HubInfo';
import { session } from '../session.svelte';
import { POLL_INTERVAL_MS } from './HubView.svelte';

function dashboard(slug: string, name: string, patch: Partial<HubDashboard> = {}): HubDashboard {
  return {
    slug,
    name,
    version: '1.2.0',
    description: `${name} description`,
    state: 'stopped',
    log_lines: [],
    source: { kind: 'local' },
    available_version: null,
    has_previous: false,
    ...patch,
  };
}

/** What the hub answers: its dashboards, changed by the actions it receives. */
let dashboards: HubDashboard[];
/** Every request the page sent. */
let requests: { method: string; url: string; token: string | null }[];
/** Answers every route with this refusal while set. */
let refusal: { status: number; code: string; message: string } | null;
/** Fails every request like a hub that quit. */
let unreachable: boolean;

function info(): HubInfo {
  return {
    eario_version: '0.1.0',
    dashboards: structuredClone(dashboards),
    last_check: null,
    available_version: null,
  };
}

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { status });
}

function find(slug: string): HubDashboard | undefined {
  return dashboards.find((entry) => entry.slug === slug);
}

async function hub(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
  if (unreachable) {
    throw new TypeError('Failed to fetch');
  }
  const url = String(input);
  const method = init?.method ?? 'GET';
  const headers = init?.headers as Record<string, string> | undefined;
  requests.push({ method, url, token: headers?.['x-eario-token'] ?? null });

  if (method === 'GET' && url === '/api/app') {
    return json({ kind: 'hub', ...info() });
  }
  if (refusal !== null) {
    return json({ code: refusal.code, message: refusal.message }, refusal.status);
  }
  if (method === 'POST' && url === '/api/stop-all') {
    for (const entry of dashboards) {
      entry.state = 'stopped';
    }
    return json(info());
  }
  const action = /^\/api\/dashboards\/([^/]+)\/(start|stop|open|uninstall)$/.exec(url);
  const entry = find(decodeURIComponent(action?.[1] ?? ''));
  if (method === 'POST' && action !== null && entry !== undefined) {
    switch (action[2]) {
      case 'start':
        entry.state = 'starting';
        return json(info());
      case 'stop':
        entry.state = 'stopped';
        return json(info());
      case 'open':
        return json({ url: `http://127.0.0.1:5000/#token=child-${entry.slug}` });
      case 'uninstall':
        dashboards = dashboards.filter((other) => other.slug !== entry.slug);
        return json(info());
    }
  }
  return json({ code: 'unknown_endpoint', message: 'not found' }, 404);
}

function card(name: string): HTMLElement {
  const heading = screen.getByRole('heading', { level: 3, name });
  const container = heading.closest('li');
  if (container === null) {
    throw new Error(`no card for ${name}`);
  }
  return container;
}

function button(name: string, action: string): HTMLButtonElement {
  return within(card(name)).getByRole('button', { name: `${action} ${name}` });
}

async function showHub(): Promise<void> {
  render(App);
  await screen.findByRole('heading', { level: 1, name: 'Eario hub' });
}

beforeEach(() => {
  dashboards = [
    dashboard('alpha', 'Alpha'),
    dashboard('beta', 'Beta', { version: null, state: 'running' }),
  ];
  requests = [];
  refusal = null;
  unreachable = false;
  // The session is shared by all tests: an earlier test may have stopped it.
  session.stopped = false;
  vi.stubGlobal('fetch', vi.fn(hub));
  window.sessionStorage.clear();
  window.history.replaceState(null, '', '/#token=hubtoken');
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe('HubView', () => {
  it('lists the installed dashboards with their version and state', async () => {
    await showHub();

    expect(screen.getByRole('list', { name: 'Installed dashboards' })).toBeTruthy();
    const alpha = within(card('Alpha'));
    expect(alpha.getByText('Version 1.2.0')).toBeTruthy();
    expect(alpha.getByText('Stopped')).toBeTruthy();
    expect(alpha.getByText('Alpha description')).toBeTruthy();
    const beta = within(card('Beta'));
    expect(beta.getByText('Version unknown')).toBeTruthy();
    expect(beta.getByText('Running')).toBeTruthy();
    expect(document.title).toBe('Eario hub');
  });

  it('enables only the actions that fit the state of each dashboard', async () => {
    dashboards = [
      dashboard('a', 'Stopped one'),
      dashboard('b', 'Starting one', { state: 'starting' }),
      dashboard('c', 'Running one', { state: 'running' }),
      dashboard('d', 'Failed one', { state: 'failed', log_lines: ['boom'] }),
    ];
    await showHub();

    const enabled = (name: string) =>
      ['Start', 'Open', 'Stop', 'Uninstall'].filter((action) => !button(name, action).disabled);
    expect(enabled('Stopped one')).toEqual(['Start', 'Uninstall']);
    expect(enabled('Starting one')).toEqual(['Stop']);
    expect(enabled('Running one')).toEqual(['Open', 'Stop']);
    expect(enabled('Failed one')).toEqual(['Start', 'Uninstall']);
  });

  it('shows the last log lines of a dashboard that failed to start', async () => {
    dashboards = [
      dashboard('broken', 'Broken', {
        state: 'failed',
        log_lines: ['starting', 'Error: address in use'],
      }),
    ];
    await showHub();

    const log = screen.getByRole('region', { name: 'Last log lines of Broken' });
    expect(log.textContent).toBe('starting\nError: address in use');
  });

  it('shows no log for a dashboard that did not fail', async () => {
    await showHub();
    expect(screen.queryByRole('group')).toBeNull();
  });

  it('says how to install a dashboard when there is none', async () => {
    dashboards = [];
    await showHub();
    expect(screen.getByText(/No dashboard is installed/)).toBeTruthy();
    expect((screen.getByRole('button', { name: 'Stop all' }) as HTMLButtonElement).disabled).toBe(
      true,
    );
  });

  it('starts a dashboard by its slug and shows the answer', async () => {
    await showHub();

    await fireEvent.click(button('Alpha', 'Start'));

    await waitFor(() => expect(within(card('Alpha')).getByText('Starting')).toBeTruthy());
    expect(requests).toContainEqual({
      method: 'POST',
      url: '/api/dashboards/alpha/start',
      token: 'hubtoken',
    });
    expect(screen.getByRole('status').textContent).toBe('Alpha is starting.');
  });

  it('stops a dashboard', async () => {
    await showHub();

    await fireEvent.click(button('Beta', 'Stop'));

    await waitFor(() => expect(within(card('Beta')).getByText('Stopped')).toBeTruthy());
    expect(requests.map((request) => request.url)).toContain('/api/dashboards/beta/stop');
    expect(screen.getByRole('status').textContent).toBe('Beta is stopped.');
  });

  it('opens a running dashboard in a new tab at the address the hub returns', async () => {
    const opened = vi.spyOn(window, 'open').mockReturnValue(null);
    await showHub();

    await fireEvent.click(button('Beta', 'Open'));

    await waitFor(() =>
      expect(opened).toHaveBeenCalledWith(
        'http://127.0.0.1:5000/#token=child-beta',
        '_blank',
        'noopener',
      ),
    );
    await waitFor(() =>
      expect(screen.getByRole('status').textContent).toBe('Beta opened in a new tab.'),
    );
  });

  it('stops every dashboard with Stop all', async () => {
    dashboards = [
      dashboard('a', 'First', { state: 'running' }),
      dashboard('b', 'Second', { state: 'starting' }),
    ];
    await showHub();

    await fireEvent.click(screen.getByRole('button', { name: 'Stop all' }));

    await waitFor(() => expect(screen.getAllByText('Stopped')).toHaveLength(2));
    expect(requests).toContainEqual({ method: 'POST', url: '/api/stop-all', token: 'hubtoken' });
    expect((screen.getByRole('button', { name: 'Stop all' }) as HTMLButtonElement).disabled).toBe(
      true,
    );
  });

  it('uninstalls a dashboard once the user confirms', async () => {
    const confirm = vi.spyOn(window, 'confirm').mockReturnValue(true);
    await showHub();

    await fireEvent.click(button('Alpha', 'Uninstall'));

    await waitFor(() => expect(screen.queryByRole('heading', { name: 'Alpha' })).toBeNull());
    expect(confirm).toHaveBeenCalledOnce();
    expect(requests.map((request) => request.url)).toContain('/api/dashboards/alpha/uninstall');
    expect(screen.getByRole('status').textContent).toBe('Alpha uninstalled.');
  });

  it('keeps a dashboard when the user declines to uninstall it', async () => {
    vi.spyOn(window, 'confirm').mockReturnValue(false);
    await showHub();

    await fireEvent.click(button('Alpha', 'Uninstall'));

    expect(requests.some((request) => request.url.endsWith('/uninstall'))).toBe(false);
    expect(card('Alpha')).toBeTruthy();
  });

  it('shows a refusal of the hub as an alert and keeps the list', async () => {
    await showHub();
    refusal = { status: 409, code: 'hub_modified', message: 'Alpha was modified since install.' };

    await fireEvent.click(button('Alpha', 'Start'));

    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toBe('Alpha was modified since install.');
    expect(within(card('Alpha')).getByText('Stopped')).toBeTruthy();
    expect(button('Alpha', 'Start').disabled).toBe(false);
  });

  it('quits the hub after a confirmation and says the dashboards keep running', async () => {
    const confirm = vi.spyOn(window, 'confirm').mockReturnValue(true);
    vi.mocked(fetch).mockImplementation(async (input, init) =>
      String(input) === '/api/shutdown' ? new Response(null, { status: 204 }) : hub(input, init),
    );
    await showHub();

    await fireEvent.click(screen.getByRole('button', { name: 'Quit hub' }));

    expect(await screen.findByRole('heading', { name: 'The hub has stopped' })).toBeTruthy();
    expect(confirm.mock.calls[0]?.[0]).toMatch(/keep running/);
  });

  it('has no theme selector, since the hub keeps no preferences', async () => {
    await showHub();
    expect(screen.queryByLabelText('Theme')).toBeNull();
  });
});

describe('HubView polling', () => {
  // Only the interval is replaced, so promises and the other timers run as usual.
  async function showHubWithClock(): Promise<void> {
    vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] });
    render(App);
    await vi.advanceTimersByTimeAsync(0);
    await tick();
  }

  async function elapse(milliseconds: number): Promise<void> {
    await vi.advanceTimersByTimeAsync(milliseconds);
    await tick();
  }

  const listRequests = () => requests.filter((request) => request.url === '/api/app').length;

  it('asks for the list every two seconds and announces a change', async () => {
    expect(POLL_INTERVAL_MS).toBe(2000);
    await showHubWithClock();
    expect(listRequests()).toBe(1);
    expect(within(card('Alpha')).getByText('Stopped')).toBeTruthy();

    const alpha = find('alpha');
    if (alpha !== undefined) {
      alpha.state = 'running';
    }
    await elapse(1999);
    expect(listRequests()).toBe(1);
    await elapse(1);

    expect(listRequests()).toBe(2);
    expect(within(card('Alpha')).getByText('Running')).toBeTruthy();
    expect(screen.getByRole('status').textContent).toBe('Alpha is running.');

    await elapse(2000);
    expect(listRequests()).toBe(3);
  });

  it('announces a dashboard that failed to start', async () => {
    await showHubWithClock();
    dashboards = [
      dashboard('alpha', 'Alpha', { state: 'failed', log_lines: ['Error: no port'] }),
      dashboard('beta', 'Beta', { version: null, state: 'running' }),
    ];

    await elapse(POLL_INTERVAL_MS);

    expect(screen.getByRole('status').textContent).toBe('Alpha failed to start.');
    expect(screen.getByRole('region', { name: 'Last log lines of Alpha' }).textContent).toBe(
      'Error: no port',
    );
  });

  it('does not ask while the page is hidden, and catches up when it is shown again', async () => {
    await showHubWithClock();
    const visibility = vi.spyOn(document, 'visibilityState', 'get').mockReturnValue('hidden');

    await elapse(POLL_INTERVAL_MS * 3);
    expect(listRequests()).toBe(1);

    visibility.mockReturnValue('visible');
    document.dispatchEvent(new Event('visibilitychange'));
    await elapse(0);
    expect(listRequests()).toBe(2);
  });

  it('stops asking when the page is left', async () => {
    await showHubWithClock();
    cleanup();

    await elapse(POLL_INTERVAL_MS * 3);

    expect(listRequests()).toBe(1);
  });

  it('shows an alert while the hub does not answer, and removes it when it does', async () => {
    await showHubWithClock();
    unreachable = true;

    await elapse(POLL_INTERVAL_MS);
    const alert = screen.getByRole('alert');
    expect(alert.textContent).toContain('The hub is not responding');
    // The last list stays on screen.
    expect(card('Alpha')).toBeTruthy();

    unreachable = false;
    await elapse(POLL_INTERVAL_MS);
    expect(screen.queryByRole('alert')).toBeNull();
  });
});
