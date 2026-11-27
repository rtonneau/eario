import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { ApiFailure, api, initToken } from './api';

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
});
