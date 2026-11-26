// Renders a whole dashboard from a specification, with the backend replaced
// by canned answers. Charts are left out: jsdom has no canvas.

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import App from './App.svelte';
import type { AppInfo } from './lib/generated/AppInfo';
import type { ComponentData } from './lib/generated/ComponentData';
import type { StateUpdate } from './lib/generated/StateUpdate';

const info: AppInfo = {
  eario_version: '0.1.0',
  revision: 1,
  preferences: { theme: 'system' },
  state: { x: 'energy_keV' },
  spec: {
    spec_version: 1,
    title: 'Test dashboard',
    datasets: [{ id: 'data', label: 'data' }],
    pages: [
      {
        id: 'page-1',
        title: 'Overview',
        rows: [
          {
            items: [
              {
                id: 'intro',
                title: 'About',
                span: 6,
                kind: { type: 'text', text: 'First paragraph.\n\nSecond paragraph.' },
              },
              {
                id: 'x',
                title: null,
                span: 6,
                kind: {
                  type: 'column_select',
                  dataset: 'data',
                  state: 'x',
                  label: 'X axis',
                  numeric_only: true,
                  default_index: 0,
                },
              },
            ],
          },
        ],
      },
      {
        id: 'page-2',
        title: 'Rows',
        rows: [
          {
            items: [
              {
                id: 'table',
                title: 'Rows',
                span: 12,
                kind: {
                  type: 'data_table',
                  view: { dataset: 'data', filter: null },
                  page_size: 2,
                },
              },
            ],
          },
        ],
      },
    ],
  },
};

const componentData: Record<string, ComponentData> = {
  intro: { type: 'text' },
  x: { type: 'column_select', options: ['energy_keV', 'counts'] },
  table: {
    type: 'data_table',
    table: {
      columns: ['sample', 'counts'],
      rows: [
        ['Si-ref', 1500],
        ['Au-thin', null],
      ],
      offset: 0,
      total_rows: 3,
      unfiltered_rows: 40,
    },
  },
};

const requests: { method: string; url: string; body: string | null }[] = [];

function json(body: unknown): Response {
  return new Response(JSON.stringify(body), { status: 200 });
}

async function backend(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
  const url = String(input);
  const method = init?.method ?? 'GET';
  requests.push({ method, url, body: typeof init?.body === 'string' ? init.body : null });

  if (url === '/api/app') {
    return json(info);
  }
  const component = /^\/api\/components\/([^/]+)\/data/.exec(url);
  if (component) {
    return json(componentData[component[1] ?? '']);
  }
  if (method === 'PUT' && url === '/api/state/x') {
    const update: StateUpdate = { revision: 2, state: { x: 'counts' }, invalidated: [] };
    return json(update);
  }
  return new Response(JSON.stringify({ code: 'unknown_endpoint', message: 'not found' }), {
    status: 404,
  });
}

beforeEach(() => {
  requests.length = 0;
  vi.stubGlobal('fetch', vi.fn(backend));
  window.sessionStorage.clear();
  window.history.replaceState(null, '', '/#token=abc123');
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('App', () => {
  it('renders the first page of the dashboard it receives', async () => {
    render(App);

    expect(await screen.findByRole('heading', { name: 'Test dashboard' })).toBeTruthy();
    expect(screen.getByText('First paragraph.')).toBeTruthy();
    expect(screen.getByText('Second paragraph.')).toBeTruthy();
    expect(document.title).toBe('Test dashboard');

    const select = (await screen.findByLabelText('X axis')) as HTMLSelectElement;
    await waitFor(() => expect(select.value).toBe('energy_keV'));
  });

  it('sends a changed selection to the backend and shows the returned state', async () => {
    render(App);
    const select = (await screen.findByLabelText('X axis')) as HTMLSelectElement;
    await waitFor(() => expect(select.options).toHaveLength(2));

    await fireEvent.change(select, { target: { value: 'counts' } });

    await waitFor(() =>
      expect(requests).toContainEqual({ method: 'PUT', url: '/api/state/x', body: '"counts"' }),
    );
    await waitFor(() => expect(select.value).toBe('counts'));
  });

  it('switches page and renders a table with formatted cells', async () => {
    render(App);
    await fireEvent.click(await screen.findByRole('button', { name: 'Rows' }));

    expect(await screen.findByRole('columnheader', { name: 'sample' })).toBeTruthy();
    expect(screen.getByText('Si-ref')).toBeTruthy();
    expect(screen.getByText('1,500')).toBeTruthy();
    expect(screen.getByText(/Rows 1–2 of\s+3/)).toBeTruthy();
    expect(screen.getByText(/filtered from 40/)).toBeTruthy();
    expect((screen.getByRole('button', { name: 'Previous' }) as HTMLButtonElement).disabled).toBe(
      true,
    );

    await fireEvent.click(screen.getByRole('button', { name: 'Next' }));
    await waitFor(() =>
      expect(requests.some((request) => request.url.endsWith('/table/data?offset=2'))).toBe(true),
    );
  });

  it('explains how to open the page when the token is missing', async () => {
    window.history.replaceState(null, '', '/');
    render(App);
    expect(await screen.findByText(/opened without its session token/)).toBeTruthy();
    expect(requests.filter((request) => request.url === '/api/app')).toHaveLength(0);
  });
});
