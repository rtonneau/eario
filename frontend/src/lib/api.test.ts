import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { ApiFailure, api, errorWithCode, initToken } from './api';

function respond(status: number, body?: unknown): Response {
  return new Response(body === undefined ? null : JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

const fetchMock = vi.fn<typeof fetch>();

beforeEach(() => {
  vi.stubGlobal('fetch', fetchMock);
  window.sessionStorage.clear();
  window.history.replaceState(null, '', '/');
});

afterEach(() => {
  fetchMock.mockReset();
  vi.unstubAllGlobals();
});

describe('initToken', () => {
  it('reports a missing token', () => {
    expect(initToken()).toBe(false);
  });

  it('takes the token from the URL fragment and removes it from the address', () => {
    window.history.replaceState(null, '', '/#token=abc123');
    expect(initToken()).toBe(true);
    expect(window.location.hash).toBe('');
  });

  it('finds the token again after a reload of the tab', () => {
    window.history.replaceState(null, '', '/#token=abc123');
    initToken();
    expect(initToken()).toBe(true);
  });
});

describe('api', () => {
  beforeEach(() => {
    window.history.replaceState(null, '', '/#token=abc123');
    initToken();
  });

  it('sends the session token with every request', async () => {
    fetchMock.mockResolvedValue(respond(200, {}));
    await api.app();
    const [url, init] = fetchMock.mock.calls[0] ?? [];
    expect(url).toBe('/api/app');
    expect((init?.headers as Record<string, string>)['x-eario-token']).toBe('abc123');
  });

  it('encodes identifiers and file names in URLs', async () => {
    fetchMock.mockResolvedValue(respond(200, {}));
    await api.upload('my data', new File(['a,b'], 'run #1.csv'));
    expect(fetchMock.mock.calls[0]?.[0]).toBe(
      '/api/datasets/my%20data/upload?filename=run%20%231.csv',
    );
  });

  it('turns an error body into a failure carrying its code and message', async () => {
    fetchMock.mockResolvedValue(
      respond(422, { code: 'parse_error', message: 'could not read csv data' }),
    );
    const failure = await api.componentData('table').catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(ApiFailure);
    expect(failure).toMatchObject({
      code: 'parse_error',
      message: 'could not read csv data',
      status: 422,
    });
  });

  it('explains an oversized upload', async () => {
    fetchMock.mockResolvedValue(new Response('length limit exceeded', { status: 413 }));
    const failure = await api.upload('data', new File(['x'], 'big.csv')).catch((error) => error);
    expect(failure).toMatchObject({ code: 'too_large' });
  });

  it('explains an unreachable backend', async () => {
    fetchMock.mockRejectedValue(new TypeError('Failed to fetch'));
    const failure = await api.app().catch((error: unknown) => error);
    expect(failure).toMatchObject({ code: 'unreachable', status: 0 });
  });

  it('returns nothing for an empty answer', async () => {
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));
    await expect(api.shutdown()).resolves.toBeUndefined();
  });

  it('calls the hub routes by slug, with the token', async () => {
    fetchMock.mockImplementation(async () => respond(200, {}));
    await api.hubStart('alpha');
    await api.hubStop('alpha');
    await api.hubOpen('alpha');
    await api.hubUninstall('alpha');
    await api.hubStopAll();
    const calls = fetchMock.mock.calls.map(([url, init]) => [init?.method, url]);
    expect(calls).toEqual([
      ['POST', '/api/dashboards/alpha/start'],
      ['POST', '/api/dashboards/alpha/stop'],
      ['POST', '/api/dashboards/alpha/open'],
      ['POST', '/api/dashboards/alpha/uninstall'],
      ['POST', '/api/stop-all'],
    ]);
    for (const [, init] of fetchMock.mock.calls) {
      expect((init?.headers as Record<string, string>)['x-eario-token']).toBe('abc123');
      expect(init?.body).toBeUndefined();
    }
  });

  it('never lets a slug change the path of a hub route', async () => {
    fetchMock.mockResolvedValue(respond(200, {}));
    await api.hubStart('../stop-all');
    expect(fetchMock.mock.calls[0]?.[0]).toBe('/api/dashboards/..%2Fstop-all/start');
  });

  it('calls the remote hub routes by repository, slug and SHA-256, without a body', async () => {
    fetchMock.mockImplementation(async () => respond(200, {}));
    const sha = 'a'.repeat(64);
    const key = 'b'.repeat(64);
    await api.hubCatalogs();
    await api.hubAddCatalog('lab/catalog');
    await api.hubRemoveCatalog('lab/catalog');
    await api.hubPreview('lab/signal-lab');
    await api.hubAcceptKey('lab/signal-lab', null, key);
    await api.hubInstall('lab/signal-lab', null, sha);
    await api.hubPreview('lab/suite', 'signal-lab');
    await api.hubAcceptKey('lab/suite', 'signal-lab', key);
    await api.hubInstall('lab/suite', 'signal-lab', sha);
    await api.hubCheck();
    await api.hubUpdatePreview('signal-lab');
    await api.hubUpdate('signal-lab', sha);
    await api.hubRollback('signal-lab');
    await api.hubSettings();
    await api.hubSetCheckAtStart(false);
    await api.hubSetCheckAtStart(true);
    await api.hubSelfUpdate();
    const calls = fetchMock.mock.calls.map(([url, init]) => [init?.method, url]);
    expect(calls).toEqual([
      ['GET', '/api/catalogs'],
      ['POST', '/api/catalogs/lab/catalog/add'],
      ['POST', '/api/catalogs/lab/catalog/remove'],
      ['POST', '/api/sources/lab/signal-lab/preview'],
      ['POST', `/api/sources/lab/signal-lab/accept-key/${key}`],
      ['POST', `/api/sources/lab/signal-lab/install/${sha}`],
      ['POST', '/api/sources/lab/suite/dashboards/signal-lab/preview'],
      ['POST', `/api/sources/lab/suite/dashboards/signal-lab/accept-key/${key}`],
      ['POST', `/api/sources/lab/suite/dashboards/signal-lab/install/${sha}`],
      ['POST', '/api/check'],
      ['POST', '/api/dashboards/signal-lab/update-preview'],
      ['POST', `/api/dashboards/signal-lab/update/${sha}`],
      ['POST', '/api/dashboards/signal-lab/rollback'],
      ['GET', '/api/settings'],
      ['POST', '/api/settings/check-at-start/off'],
      ['POST', '/api/settings/check-at-start/on'],
      ['POST', '/api/self-update'],
    ]);
    for (const [, init] of fetchMock.mock.calls) {
      expect((init?.headers as Record<string, string>)['x-eario-token']).toBe('abc123');
      expect(init?.body).toBeUndefined();
    }
  });

  it('keeps a repository to two path segments, and a slug to one', async () => {
    fetchMock.mockImplementation(async () => respond(200, {}));
    await api.hubPreview('lab/../../stop-all');
    await api.hubPreview('lab/suite', '../../stop-all');
    expect(fetchMock.mock.calls.map(([url]) => url)).toEqual([
      '/api/sources/lab/..%2F..%2Fstop-all/preview',
      '/api/sources/lab/suite/dashboards/..%2F..%2Fstop-all/preview',
    ]);
  });

  // The URL parser drops a `.` segment and climbs on a `..` one, encoded or
  // not, so such a name would reach another route.
  it('refuses a repository with an empty, `.` or `..` part, before any request', () => {
    fetchMock.mockImplementation(async () => respond(200, {}));
    for (const repo of ['nothing', 'lab/', '/repo', '', 'lab/..', '../repo', 'lab/.', './repo']) {
      expect(() => api.hubPreview(repo), repo).toThrow(ApiFailure);
      expect(() => api.hubAddCatalog(repo), repo).toThrow(
        `"${repo}" is not a repository of the form owner/repo.`,
      );
    }
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('refuses an empty, `.` or `..` name in any route, before any request', () => {
    fetchMock.mockImplementation(async () => respond(200, {}));
    for (const name of ['', '.', '..']) {
      expect(() => api.hubStart(name), name).toThrow(ApiFailure);
      expect(() => api.componentData(name), name).toThrow(ApiFailure);
      expect(() => api.setState(name, 1), name).toThrow(ApiFailure);
      expect(() => api.hubUpdate('alpha', name), name).toThrow(ApiFailure);
      expect(() => api.hubAcceptKey('lab/alpha', null, name), name).toThrow(ApiFailure);
      expect(() => api.hubPreview('lab/alpha', name), name).toThrow(ApiFailure);
      expect(() => api.hubInstall('lab/alpha', name, 'a'.repeat(64)), name).toThrow(ApiFailure);
    }
    let failure: unknown;
    try {
      void api.hubUninstall('..');
    } catch (error) {
      failure = error;
    }
    expect(failure).toMatchObject({ code: 'invalid_path_segment', status: 0 });
    expect(errorWithCode(failure)).toBe(
      'The name ".." cannot be sent to the application: a name must not be empty, "." or "..". (invalid_path_segment)',
    );
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('accepts names that only contain dots among other characters', async () => {
    fetchMock.mockImplementation(async () => respond(200, {}));
    await api.hubStart('...');
    await api.hubPreview('lab/signal.lab');
    await api.upload('data', new File(['x'], '.'));
    expect(fetchMock.mock.calls.map(([url]) => url)).toEqual([
      '/api/dashboards/.../start',
      '/api/sources/lab/signal.lab/preview',
      '/api/datasets/data/upload?filename=.',
    ]);
  });
});

describe('errorWithCode', () => {
  it('adds the code of a refusal to its message', () => {
    expect(errorWithCode(new ApiFailure('Key changed.', 'hub_key_changed', 409))).toBe(
      'Key changed. (hub_key_changed)',
    );
    expect(errorWithCode(new Error('boom'))).toBe('boom');
    expect(errorWithCode('plain')).toBe('plain');
  });
});
