// Renders the hub's page through the whole application, with the hub replaced
// by a small in-memory model that answers the routes of its HTTP API.

import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import App from '../../App.svelte';
import type { HubCatalog } from '../generated/HubCatalog';
import type { HubDashboard } from '../generated/HubDashboard';
import type { HubInfo } from '../generated/HubInfo';
import type { HubSettings } from '../generated/HubSettings';
import type { InstallPreview } from '../generated/InstallPreview';
import type { KeyStatus } from '../generated/KeyStatus';
import type { UpdateCheck } from '../generated/UpdateCheck';
import { session } from '../session.svelte';
import { POLL_INTERVAL_MS } from './HubView.svelte';
import { CONFIRM_DELAY_MS } from './InstallDialog.svelte';

// jsdom has <dialog> without its methods: open and close it as a browser
// does, without the top layer.
HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) {
  this.setAttribute('open', '');
};
HTMLDialogElement.prototype.close = function (this: HTMLDialogElement) {
  if (this.open) {
    this.removeAttribute('open');
    this.dispatchEvent(new Event('close'));
  }
};

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

const SHA = 'a'.repeat(64);
const KEY_SHA = 'b'.repeat(64);
const KEY = 'RWQBAgMEBQYHCAkKCwwNDg8QERITFBUWFxgZGhscHR4fICEiIyQlJicoKQ==';

/** What the hub answers for a release of `repo`: the preview and, once trusted, its manifest. */
function preview(repo: string, name: string, status: KeyStatus, version = '1.0.0'): InstallPreview {
  const trusted = status !== 'unknown' && status !== 'changed';
  const slug = repo.split('/')[1] ?? repo;
  return {
    repo,
    slug,
    manifest: trusted
      ? { name, slug, version, eario_version: '0.2.0', protocol: 1, description: '' }
      : null,
    tag: `v${version}`,
    asset: `${slug}-x86_64-pc-windows-msvc.exe`,
    size: 54_321_664,
    sha256: SHA,
    public_key: KEY,
    key_sha256: KEY_SHA,
    key_status: status,
  };
}

/** What the hub answers: its dashboards, changed by the actions it receives. */
let dashboards: HubDashboard[];
/** Every request the page sent. */
let requests: { method: string; url: string; token: string | null }[];
/** Answers every POST with this refusal while set. */
let refusal: { status: number; code: string; message: string } | null;
/** Fails every request like a hub that quit. */
let unreachable: boolean;
let catalogs: HubCatalog[];
let settings: HubSettings;
let lastCheck: UpdateCheck | null;
/** A newer hub the last check found. */
let hubAvailable: string | null;
/** The latest release of each repository, as a preview. */
let releases: Record<string, InstallPreview>;
/** The update preview of each slug. */
let updates: Record<string, InstallPreview>;
/** Version restored by a rollback, by slug. */
let previousVersions: Record<string, string | null>;

function info(): HubInfo {
  return {
    eario_version: '0.2.0',
    dashboards: structuredClone(dashboards),
    last_check: structuredClone(lastCheck),
    available_version: hubAvailable,
  };
}

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { status });
}

function find(slug: string): HubDashboard | undefined {
  return dashboards.find((entry) => entry.slug === slug);
}

function refuse(status: number, code: string, message = code): Response {
  return json({ code, message }, status);
}

/** The remote routes: catalogs, previews, installs, updates, checks, settings. */
function remote(method: string, url: string): Response | null {
  if (method === 'GET' && url === '/api/catalogs') {
    return json(catalogs);
  }
  if (method === 'GET' && url === '/api/settings') {
    return json(settings);
  }
  if (method !== 'POST') {
    return null;
  }
  const setting = /^\/api\/settings\/check-at-start\/(on|off)$/.exec(url);
  if (setting !== null) {
    settings = { check_updates_at_start: setting[1] === 'on' };
    return json(settings);
  }
  const catalog = /^\/api\/catalogs\/([^/]+\/[^/]+)\/(add|remove)$/.exec(url);
  if (catalog !== null) {
    const repo = catalog[1] ?? '';
    if (catalog[2] === 'add') {
      catalogs = [...catalogs, { repo, name: 'Added', dashboards: [], error: null }];
    } else {
      catalogs = catalogs.filter((entry) => entry.repo !== repo);
    }
    return json(catalogs);
  }
  const source = /^\/api\/sources\/([^/]+\/[^/]+)\/(preview|accept-key|install)(?:\/([0-9a-f]+))?$/.exec(
    url,
  );
  if (source !== null) {
    const release = releases[source[1] ?? ''];
    if (release === undefined) {
      return refuse(502, 'github_not_found', 'No such repository.');
    }
    switch (source[2]) {
      case 'preview':
        return json(release);
      case 'accept-key':
        if (source[3] !== release.key_sha256) {
          return refuse(409, 'hub_key_mismatch');
        }
        release.key_status = 'accepted';
        release.manifest = preview(release.repo, release.repo, 'accepted').manifest;
        return json(release);
      case 'install': {
        if (release.manifest === null) {
          return refuse(409, 'hub_key_not_accepted');
        }
        if (source[3] !== release.sha256) {
          return refuse(409, 'hub_preview_mismatch');
        }
        const { slug, name, version } = release.manifest;
        dashboards.push(
          dashboard(slug, name, { version, source: { kind: 'github', repo: release.repo } }),
        );
        return json(info());
      }
    }
  }
  if (url === '/api/check') {
    lastCheck = {
      checked_at: (lastCheck?.checked_at ?? 1_795_780_800) + 60,
      dashboards: dashboards
        .filter((entry) => entry.source.kind === 'github')
        .map((entry) => ({
          slug: entry.slug,
          repo: entry.source.kind === 'github' ? entry.source.repo : '',
          latest_version: null,
          available_version: null,
          error: { code: 'github_rate_limited', message: 'GitHub limits requests for now.' },
        })),
      hub: null,
    };
    return json(info());
  }
  if (url === '/api/self-update') {
    return json({ previous_version: '0.2.0', version: hubAvailable ?? '0.2.0' });
  }
  const update = /^\/api\/dashboards\/([^/]+)\/(update-preview|update|rollback)(?:\/([0-9a-f]+))?$/.exec(
    url,
  );
  const entry = find(update?.[1] ?? '');
  if (update === null || entry === undefined) {
    return null;
  }
  switch (update[2]) {
    case 'update-preview': {
      const found = updates[entry.slug];
      return found === undefined ? refuse(409, 'hub_not_newer') : json(found);
    }
    case 'update': {
      const found = updates[entry.slug];
      if (found?.manifest == null || update[3] !== found.sha256) {
        return refuse(409, 'hub_preview_mismatch');
      }
      previousVersions[entry.slug] = entry.version;
      entry.version = found.manifest.version;
      entry.available_version = null;
      entry.has_previous = true;
      return json(info());
    }
    case 'rollback':
      entry.version = previousVersions[entry.slug] ?? null;
      entry.has_previous = false;
      return json(info());
  }
  return null;
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
  if (refusal !== null && method === 'POST') {
    return json({ code: refusal.code, message: refusal.message }, refusal.status);
  }
  const answer = remote(method, url);
  if (answer !== null) {
    return answer;
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

/** The page's own status region, outside the dialog. */
function pageStatus(): HTMLElement {
  return within(screen.getByRole('main')).getByRole('status');
}

const posted = (path: string) =>
  requests.some((request) => request.method === 'POST' && request.url === path);

beforeEach(() => {
  dashboards = [
    dashboard('alpha', 'Alpha'),
    dashboard('beta', 'Beta', { version: null, state: 'running' }),
  ];
  requests = [];
  refusal = null;
  unreachable = false;
  catalogs = [];
  settings = { check_updates_at_start: true };
  lastCheck = null;
  hubAvailable = null;
  releases = {};
  updates = {};
  previousVersions = {};
  // The session is shared by all tests: an earlier test may have stopped it,
  // or left its list, which would show the page once before it loads.
  session.stopped = false;
  session.hub = null;
  session.hubRestart = null;
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
    expect(alpha.getByText('Installed from a local executable')).toBeTruthy();
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

  it('shows a refusal of the hub as an alert, with its code, and keeps the list', async () => {
    await showHub();
    refusal = { status: 409, code: 'hub_modified', message: 'Alpha was modified since install.' };

    await fireEvent.click(button('Alpha', 'Start'));

    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toBe('Alpha was modified since install. (hub_modified)');
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

describe('HubView catalogs and installs', () => {
  beforeEach(() => {
    dashboards = [
      dashboard('alpha', 'Alpha', { source: { kind: 'github', repo: 'Lab/Alpha' } }),
    ];
    catalogs = [
      {
        repo: 'lab/catalog',
        name: 'Lab',
        dashboards: [
          { repo: 'lab/explorer', name: 'Explorer', description: 'Explore', installed_slug: null },
          { repo: 'lab/alpha', name: 'Alpha', description: '', installed_slug: 'alpha' },
        ],
        error: null,
      },
      {
        repo: 'lab/broken',
        name: null,
        dashboards: [],
        error: { code: 'github_not_found', message: 'No catalog.json.' },
      },
    ];
    releases = {
      'lab/explorer': preview('lab/explorer', 'Explorer', 'catalog'),
      'lab/unknown': preview('lab/unknown', 'Unknown', 'unknown'),
    };
  });

  async function dialog(): Promise<HTMLElement> {
    return screen.findByRole('dialog');
  }

  it('lists the catalogs, their dashboards and which are installed', async () => {
    await showHub();

    const lab = within(await screen.findByRole('list', { name: 'Dashboards of Lab' }));
    expect(lab.getByRole('button', { name: 'Install Explorer from lab/explorer' })).toBeTruthy();
    // Installed from the same repository, in other case.
    expect(lab.queryByRole('button', { name: /Install Alpha/ })).toBeNull();
    expect(lab.getByText('Installed')).toBeTruthy();
    expect(screen.getByText('This catalog could not be read: No catalog.json. (github_not_found)'))
      .toBeTruthy();
    expect(within(card('Alpha')).getByText('From GitHub: Lab/Alpha')).toBeTruthy();
    // The catalogs are read once, not polled.
    expect(requests.filter((request) => request.url === '/api/catalogs')).toHaveLength(1);
  });

  it('adds a catalog by its repository, checking its shape first, and removes one', async () => {
    const confirm = vi.spyOn(window, 'confirm').mockReturnValue(true);
    await showHub();
    await screen.findByRole('list', { name: 'Dashboards of Lab' });
    const input = screen.getByLabelText('Catalog repository (owner/repo)') as HTMLInputElement;

    await fireEvent.input(input, { target: { value: 'not a repository' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Add catalog' }));
    expect(screen.getByRole('alert').textContent).toMatch(/owner\/repo/);
    expect(input.getAttribute('aria-invalid')).toBe('true');
    expect(requests.some((request) => request.url.includes('/add'))).toBe(false);

    await fireEvent.input(input, { target: { value: 'lab/more' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Add catalog' }));
    await waitFor(() => expect(pageStatus().textContent).toBe('Catalog lab/more added.'));
    expect(posted('/api/catalogs/lab/more/add')).toBe(true);
    expect(screen.getByRole('heading', { level: 4, name: 'Added' })).toBeTruthy();
    expect(input.value).toBe('');

    await fireEvent.click(screen.getByRole('button', { name: 'Remove catalog lab/catalog' }));
    await waitFor(() => expect(pageStatus().textContent).toBe('Catalog lab/catalog removed.'));
    expect(confirm).toHaveBeenCalledOnce();
    expect(posted('/api/catalogs/lab/catalog/remove')).toBe(true);
    expect(screen.queryByRole('heading', { level: 4, name: 'Lab' })).toBeNull();
  });

  it('installs from a catalog once the user confirmed what the hub verified', async () => {
    await showHub();
    const install = await screen.findByRole('button', {
      name: 'Install Explorer from lab/explorer',
    });

    await fireEvent.click(install);

    const found = within(await dialog());
    expect(found.getByRole('heading', { name: 'Install Explorer?' })).toBeTruthy();
    for (const text of ['lab/explorer', 'v1.0.0', 'Explorer, version 1.0.0', SHA, KEY, KEY_SHA]) {
      expect(found.getByText(text)).toBeTruthy();
    }
    expect(found.getByText('51.8 MiB (54,321,664 bytes)')).toBeTruthy();
    expect(found.getByRole('heading', { name: 'Trusted key' })).toBeTruthy();
    expect(found.queryByRole('button', { name: 'Trust this key' })).toBeNull();
    expect(document.activeElement).toBe(found.getByRole('button', { name: 'Cancel' }));
    expect(posted('/api/sources/lab/explorer/preview')).toBe(true);

    await fireEvent.click(found.getByRole('button', { name: 'Install' }));

    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(posted(`/api/sources/lab/explorer/install/${SHA}`)).toBe(true);
    expect(pageStatus().textContent).toBe('Explorer installed.');
    expect(within(card('Explorer')).getByText('From GitHub: lab/explorer')).toBeTruthy();
    expect(
      screen.queryByRole('button', { name: 'Install Explorer from lab/explorer' }),
    ).toBeNull();
    // Its Install button is gone, so focus is on the page, not lost.
    expect(document.activeElement).toBe(screen.getByRole('main'));
  });

  it('asks to trust an unknown key before it reads the dashboard and installs it', async () => {
    await showHub();
    const input = screen.getByLabelText('Repository (owner/repo)');
    await fireEvent.input(input, { target: { value: ' lab/unknown ' } });

    await fireEvent.click(screen.getByRole('button', { name: 'Add repository' }));

    const found = within(await dialog());
    expect(found.getByRole('heading', { name: 'Install lab/unknown?' })).toBeTruthy();
    expect(found.getByRole('heading', { name: 'Unknown key' })).toBeTruthy();
    expect(found.getByText(/Compare the key above/)).toBeTruthy();
    expect(found.getByText('Read from the executable once you trust its key')).toBeTruthy();
    expect(found.queryByRole('button', { name: 'Install' })).toBeNull();
    expect(found.getByText(KEY)).toBeTruthy();

    await fireEvent.click(found.getByRole('button', { name: 'Trust this key' }));

    await waitFor(() => expect(found.getByRole('heading', { name: 'Key trusted by you' })).toBeTruthy());
    expect(posted(`/api/sources/lab/unknown/accept-key/${KEY_SHA}`)).toBe(true);
    expect(found.getByText('lab/unknown, version 1.0.0')).toBeTruthy();
    expect(found.getByRole('status').textContent).toBe('Key trusted. lab/unknown, version 1.0.0.');
    expect(document.activeElement).toBe(found.getByRole('button', { name: 'Cancel' }));
    expect(posted(`/api/sources/lab/unknown/install/${SHA}`)).toBe(false);
    const confirm = found.getByRole('button', { name: 'Install' }) as HTMLButtonElement;
    await waitFor(() => expect(confirm.disabled).toBe(false));

    await fireEvent.click(confirm);

    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(posted(`/api/sources/lab/unknown/install/${SHA}`)).toBe(true);
  });

  it('does not send a repository that is not owner/repo', async () => {
    await showHub();
    await fireEvent.input(screen.getByLabelText('Repository (owner/repo)'), {
      target: { value: 'https://github.com/lab/unknown' },
    });

    await fireEvent.click(screen.getByRole('button', { name: 'Add repository' }));

    expect(screen.getByRole('alert').textContent).toMatch(/owner\/repo/);
    expect(requests.some((request) => request.url.includes('/sources/'))).toBe(false);
  });

  it('shows a refused preview where it was asked for, with its code', async () => {
    await showHub();
    await fireEvent.input(screen.getByLabelText('Repository (owner/repo)'), {
      target: { value: 'lab/nothing' },
    });

    await fireEvent.click(screen.getByRole('button', { name: 'Add repository' }));

    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toBe('No such repository. (github_not_found)');
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  /** Holds every preview at the hub until the returned function is called. */
  function holdPreviews(): () => void {
    let release = () => {};
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    vi.mocked(fetch).mockImplementation(async (input, init) => {
      if (String(input).endsWith('preview')) {
        await gate;
      }
      return hub(input, init);
    });
    return release;
  }

  /** The previews the page asked for, counted at fetch, held or not. */
  const previewsAsked = () =>
    vi
      .mocked(fetch)
      .mock.calls.map(([input]) => String(input))
      .filter((url) => url.endsWith('preview'));

  it('runs one preview at a time, and confirms only the one the user reviews', async () => {
    dashboards = [
      dashboard('alpha', 'Alpha', {
        source: { kind: 'github', repo: 'Lab/Alpha' },
        available_version: '2.0.0',
      }),
    ];
    const updateSha = 'c'.repeat(64);
    updates = { alpha: { ...preview('lab/alpha', 'Alpha', 'pinned', '2.0.0'), sha256: updateSha } };
    const release = holdPreviews();
    await showHub();
    const install = (await screen.findByRole('button', {
      name: 'Install Explorer from lab/explorer',
    })) as HTMLButtonElement;

    await fireEvent.click(install);

    // While the hub verifies Explorer, nothing else can be previewed.
    await waitFor(() => expect(install.textContent?.trim()).toBe('Verifying…'));
    expect(install.disabled).toBe(true);
    expect(button('Alpha', 'Update').disabled).toBe(true);
    const add = screen.getByRole('button', { name: 'Add repository' }) as HTMLButtonElement;
    expect(add.disabled).toBe(true);
    // Asked anyway: the form submitted by script, a click on the disabled Update.
    await fireEvent.input(screen.getByLabelText('Repository (owner/repo)', { exact: true }), {
      target: { value: 'lab/unknown' },
    });
    const form = add.closest('form');
    if (form === null) {
      throw new Error('no form around Add repository');
    }
    await fireEvent.submit(form);
    await fireEvent.click(button('Alpha', 'Update'));
    release();

    const found = within(await screen.findByRole('dialog'));
    expect(found.getByRole('heading', { name: 'Install Explorer?' })).toBeTruthy();
    expect(found.getByText(SHA)).toBeTruthy();
    expect(previewsAsked()).toEqual(['/api/sources/lab/explorer/preview']);
    // Still Explorer once every answer is in.
    await tick();
    expect(found.getByRole('heading', { name: 'Install Explorer?' })).toBeTruthy();

    await fireEvent.click(found.getByRole('button', { name: 'Install' }));

    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(posted(`/api/sources/lab/explorer/install/${SHA}`)).toBe(true);
    expect(requests.some((request) => request.url.includes('/update/'))).toBe(false);
    expect(button('Alpha', 'Update').disabled).toBe(false);
  });

  it('keeps every Install control disabled while an update is previewed', async () => {
    dashboards = [
      dashboard('alpha', 'Alpha', {
        source: { kind: 'github', repo: 'Lab/Alpha' },
        available_version: '2.0.0',
      }),
    ];
    updates = { alpha: preview('lab/alpha', 'Alpha', 'pinned', '2.0.0') };
    const release = holdPreviews();
    await showHub();
    const install = (await screen.findByRole('button', {
      name: 'Install Explorer from lab/explorer',
    })) as HTMLButtonElement;

    await fireEvent.click(button('Alpha', 'Update'));

    await waitFor(() => expect(install.disabled).toBe(true));
    expect(
      (screen.getByRole('button', { name: 'Add repository' }) as HTMLButtonElement).disabled,
    ).toBe(true);
    await fireEvent.click(install);
    release();

    const found = within(await screen.findByRole('dialog'));
    expect(found.getByRole('heading', { name: 'Update Alpha?' })).toBeTruthy();
    expect(previewsAsked()).toEqual(['/api/dashboards/alpha/update-preview']);
  });

  it('lists a catalog that names a repository twice without breaking the page', async () => {
    catalogs = [
      {
        repo: 'lab/catalog',
        name: 'Lab',
        dashboards: [
          { repo: 'lab/explorer', name: 'Explorer', description: '', installed_slug: null },
          { repo: 'lab/explorer', name: 'Explorer again', description: '', installed_slug: null },
        ],
        error: null,
      },
      { repo: 'lab/catalog', name: 'Lab twice', dashboards: [], error: null },
    ];
    await showHub();

    const lab = within(await screen.findByRole('list', { name: 'Dashboards of Lab' }));
    expect(lab.getByRole('button', { name: 'Install Explorer from lab/explorer' })).toBeTruthy();
    expect(lab.getByRole('button', { name: 'Install Explorer again from lab/explorer' }))
      .toBeTruthy();
    expect(screen.getByRole('heading', { level: 4, name: 'Lab twice' })).toBeTruthy();
    expect(card('Alpha')).toBeTruthy();
  });

  it('keeps Install disabled for a moment after the key is trusted', async () => {
    await showHub();
    await fireEvent.input(screen.getByLabelText('Repository (owner/repo)', { exact: true }), {
      target: { value: 'lab/unknown' },
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Add repository' }));
    const found = within(await dialog());
    // Only the timeouts: the answers of the hub still arrive as usual.
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });

    await fireEvent.click(found.getByRole('button', { name: 'Trust this key' }));
    // The answer arrives without the clock moving.
    const shown = () => found.queryByRole('button', { name: 'Install' }) !== null;
    for (let round = 0; round < 50 && !shown(); round++) {
      await vi.advanceTimersByTimeAsync(0);
    }

    // A second click of a double-click lands where Trust this key was.
    const confirm = found.getByRole('button', { name: 'Install' }) as HTMLButtonElement;
    expect(confirm.disabled).toBe(true);
    await fireEvent.click(confirm);
    expect(posted(`/api/sources/lab/unknown/install/${SHA}`)).toBe(false);
    await vi.advanceTimersByTimeAsync(CONFIRM_DELAY_MS - 1);
    expect(confirm.disabled).toBe(true);
    await vi.advanceTimersByTimeAsync(1);
    await tick();
    expect(confirm.disabled).toBe(false);
    vi.useRealTimers();

    await fireEvent.click(confirm);

    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(posted(`/api/sources/lab/unknown/install/${SHA}`)).toBe(true);
  });

  it('shows a refused install in the dialog and keeps it open', async () => {
    await showHub();
    await fireEvent.click(
      await screen.findByRole('button', { name: 'Install Explorer from lab/explorer' }),
    );
    const found = within(await dialog());
    refusal = { status: 409, code: 'hub_slug_taken', message: 'explorer is installed already.' };

    await fireEvent.click(found.getByRole('button', { name: 'Install' }));

    expect((await found.findByRole('alert')).textContent).toBe(
      'explorer is installed already. (hub_slug_taken)',
    );
    expect(screen.getByRole('dialog')).toBeTruthy();
  });
});

describe('HubView updates', () => {
  beforeEach(() => {
    dashboards = [
      dashboard('alpha', 'Alpha', {
        source: { kind: 'github', repo: 'lab/alpha' },
        available_version: '2.0.0',
      }),
      dashboard('local', 'Local', { available_version: '9.0.0', has_previous: true }),
    ];
    updates = { alpha: preview('lab/alpha', 'Alpha', 'pinned', '2.0.0') };
  });

  it('shows the available version and Update only for a dashboard from GitHub', async () => {
    await showHub();

    expect(within(card('Alpha')).getByText('v2.0.0 available')).toBeTruthy();
    expect(button('Alpha', 'Update').disabled).toBe(false);
    expect(within(card('Alpha')).queryByRole('button', { name: 'Roll back Alpha' })).toBeNull();
    const local = within(card('Local'));
    expect(local.queryByText(/available/)).toBeNull();
    expect(local.queryByRole('button', { name: 'Update Local' })).toBeNull();
    expect(local.queryByRole('button', { name: 'Roll back Local' })).toBeNull();
  });

  it('updates through the confirmation dialog, then rolls back after a confirmation', async () => {
    const confirm = vi.spyOn(window, 'confirm').mockReturnValue(true);
    await showHub();
    const update = button('Alpha', 'Update');

    await fireEvent.click(update);

    const found = within(await screen.findByRole('dialog'));
    expect(found.getByRole('heading', { name: 'Update Alpha?' })).toBeTruthy();
    expect(found.getByText('The same key signed the version you have installed.')).toBeTruthy();
    await fireEvent.click(found.getByRole('button', { name: 'Update' }));

    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(posted('/api/dashboards/alpha/update-preview')).toBe(true);
    expect(posted(`/api/dashboards/alpha/update/${SHA}`)).toBe(true);
    expect(pageStatus().textContent).toBe('Alpha updated to version 2.0.0.');
    expect(within(card('Alpha')).getByText('Version 2.0.0')).toBeTruthy();
    expect(within(card('Alpha')).queryByText('v2.0.0 available')).toBeNull();

    await fireEvent.click(button('Alpha', 'Roll back'));

    await waitFor(() =>
      expect(pageStatus().textContent).toBe('Alpha rolled back to version 1.2.0.'),
    );
    expect(confirm).toHaveBeenCalledOnce();
    expect(posted('/api/dashboards/alpha/rollback')).toBe(true);
    expect(within(card('Alpha')).queryByRole('button', { name: 'Roll back Alpha' })).toBeNull();
  });

  it('keeps the version when the user declines to roll back', async () => {
    vi.spyOn(window, 'confirm').mockReturnValue(false);
    dashboards = [
      dashboard('alpha', 'Alpha', { source: { kind: 'github', repo: 'lab/alpha' }, has_previous: true }),
    ];
    await showHub();

    await fireEvent.click(button('Alpha', 'Roll back'));

    expect(posted('/api/dashboards/alpha/rollback')).toBe(false);
  });

  it('warns loudly about a changed key, and Escape cancels the update', async () => {
    updates = { alpha: preview('lab/alpha', 'Alpha', 'changed', '2.0.0') };
    await showHub();
    const update = button('Alpha', 'Update');
    update.focus();

    await fireEvent.click(update);

    const element = await screen.findByRole('dialog');
    const found = within(element);
    const alarm = found.getByRole('alert', { name: 'Warning: the signing key has changed' });
    expect(alarm.textContent).toMatch(/Someone else may be publishing in its name/);
    expect(found.queryByRole('button', { name: 'Update' })).toBeNull();
    expect(found.getByRole('button', { name: 'Trust this key' })).toBeTruthy();

    // What the browser does on Escape.
    await fireEvent(element, new Event('cancel', { cancelable: true }));

    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(pageStatus().textContent).toBe('Nothing was updated.');
    expect(document.activeElement).toBe(update);
    expect(requests.some((request) => /\/update\/|accept-key/.test(request.url))).toBe(false);
    expect(within(card('Alpha')).getByText('Version 1.2.0')).toBeTruthy();
  });

  it('checks now, and says what could not be checked with the message and code', async () => {
    await showHub();
    expect(screen.getByText('Not checked yet.')).toBeTruthy();

    await fireEvent.click(screen.getByRole('button', { name: 'Check now' }));

    await waitFor(() =>
      expect(pageStatus().textContent).toMatch(/^Checked for updates at .+ Alpha could not be checked\.$/),
    );
    expect(posted('/api/check')).toBe(true);
    const problems = within(screen.getByRole('list', { name: 'Could not check' }));
    expect(
      problems.getByText(
        'Alpha could not be checked: GitHub limits requests for now. (github_rate_limited)',
      ),
    ).toBeTruthy();
    expect(screen.getByText(/^Last checked /)).toBeTruthy();
  });

  it('turns the check at start off and on, as the hub stores it', async () => {
    await showHub();
    const toggle = screen.getByLabelText('Check for updates at start') as HTMLInputElement;
    await waitFor(() => expect(toggle.disabled).toBe(false));
    expect(toggle.checked).toBe(true);

    await fireEvent.click(toggle);

    await waitFor(() =>
      expect(pageStatus().textContent).toBe('The hub will not check for updates when it starts.'),
    );
    expect(posted('/api/settings/check-at-start/off')).toBe(true);
    expect(toggle.checked).toBe(false);

    await fireEvent.click(toggle);
    await waitFor(() => expect(posted('/api/settings/check-at-start/on')).toBe(true));
    expect(toggle.checked).toBe(true);
  });

  it('puts the setting back when the hub refuses it', async () => {
    await showHub();
    const toggle = screen.getByLabelText('Check for updates at start') as HTMLInputElement;
    await waitFor(() => expect(toggle.disabled).toBe(false));
    refusal = { status: 500, code: 'hub_dir_write', message: 'Cannot write settings.json.' };

    await fireEvent.click(toggle);

    expect((await screen.findByRole('alert')).textContent).toBe(
      'Cannot write settings.json. (hub_dir_write)',
    );
    expect(toggle.checked).toBe(true);
  });

  it('shows a refused self-update with its code, and keeps the page', async () => {
    vi.spyOn(window, 'confirm').mockReturnValue(true);
    hubAvailable = '0.3.0';
    await showHub();
    refusal = { status: 409, code: 'self_update_not_writable', message: 'Cannot write C:/hub.' };

    await fireEvent.click(screen.getByRole('button', { name: 'Update the hub to v0.3.0 and restart' }));

    expect((await screen.findByRole('alert')).textContent).toBe(
      'Cannot write C:/hub. (self_update_not_writable)',
    );
    expect(screen.queryByRole('heading', { name: 'The hub is restarting' })).toBeNull();
    expect(card('Alpha')).toBeTruthy();
  });

  it('says when this build of the hub does not update itself', async () => {
    lastCheck = {
      checked_at: 1_795_780_800,
      dashboards: [],
      hub: {
        repo: 'rtonneau/eario',
        latest_version: null,
        available_version: null,
        error: { code: 'self_update_not_configured', message: 'No release key.' },
      },
    };
    await showHub();

    expect(screen.getByText('This build of the hub does not update itself.')).toBeTruthy();
    expect(screen.queryByRole('list', { name: 'Could not check' })).toBeNull();
    expect(screen.queryByRole('button', { name: /Update the hub/ })).toBeNull();
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

  it('announces a check the hub ran by itself, such as the one at start', async () => {
    await showHubWithClock();
    lastCheck = { checked_at: 1_795_780_800, dashboards: [], hub: null };

    await elapse(POLL_INTERVAL_MS);

    expect(screen.getByRole('status').textContent).toMatch(
      /^Checked for updates at .+\. No update available\.$/,
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

  it('updates the hub itself after a confirmation, then stops asking the old address', async () => {
    const confirm = vi.spyOn(window, 'confirm').mockReturnValue(true);
    hubAvailable = '0.3.0';
    await showHubWithClock();
    expect(screen.getByText('Eario hub v0.3.0 is available. This hub runs v0.2.0.')).toBeTruthy();

    await fireEvent.click(screen.getByRole('button', { name: 'Update the hub to v0.3.0 and restart' }));
    await vi.waitFor(() =>
      expect(screen.getByRole('heading', { name: 'The hub is restarting' })).toBeTruthy(),
    );

    expect(confirm.mock.calls[0]?.[0]).toMatch(/new address/);
    expect(posted('/api/self-update')).toBe(true);
    expect(screen.getByRole('status').textContent).toBe(
      'The hub is updated to v0.3.0. It restarts at a new address and opens it in a new tab.',
    );
    expect(screen.getByText(/updated from v0\.2\.0 to v0\.3\.0\. It starts again at a new address/))
      .toBeTruthy();
    // Counted at fetch, since the old address no longer answers.
    const attempts = () => vi.mocked(fetch).mock.calls.length;
    const before = attempts();
    unreachable = true;

    await elapse(POLL_INTERVAL_MS * 3);

    expect(attempts()).toBe(before);
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('does not update the hub when the user declines', async () => {
    vi.spyOn(window, 'confirm').mockReturnValue(false);
    hubAvailable = '0.3.0';
    await showHubWithClock();

    await fireEvent.click(screen.getByRole('button', { name: 'Update the hub to v0.3.0 and restart' }));
    await elapse(0);

    expect(posted('/api/self-update')).toBe(false);
    expect(screen.queryByRole('heading', { name: 'The hub is restarting' })).toBeNull();
  });
});
