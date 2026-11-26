// Test support for the input components: a fake backend behind `fetch`, so
// the components are exercised through `session` and `api` as in the app.

import { vi } from 'vitest';

import type { ComponentData } from '../generated/ComponentData';
import type { ComponentKind } from '../generated/ComponentKind';
import type { ComponentSpec } from '../generated/ComponentSpec';
import type { JsonValue } from '../generated/serde_json/JsonValue';
import { session } from '../session.svelte';

export const fetchMock = vi.fn<typeof fetch>();

function respond(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

/** A component of `kind`, named `input`. */
export function component(kind: ComponentKind): ComponentSpec {
  return { id: 'input', title: null, span: 4, kind };
}

/**
 * Answers component data requests with `data`, and state changes by
 * accepting the value, or refusing it with `refusal` as message.
 */
export function fakeBackend({
  data = { type: 'input' },
  refusal,
}: { data?: ComponentData; refusal?: string } = {}): void {
  fetchMock.mockImplementation(async (input, init) => {
    const url = String(input);
    if (init?.method !== 'PUT') {
      return respond(200, data);
    }
    if (refusal !== undefined) {
      return respond(422, { code: 'invalid_state_value', message: refusal });
    }
    const key = decodeURIComponent(url.slice('/api/state/'.length));
    const value = JSON.parse(String(init.body)) as JsonValue;
    return respond(200, { revision: 1, state: { ...session.state, [key]: value }, invalidated: [] });
  });
}

/** The state changes sent so far, as `[key, value]` pairs. */
export function sentChanges(): [string, unknown][] {
  return fetchMock.mock.calls
    .filter(([, init]) => init?.method === 'PUT')
    .map(([input, init]) => [
      decodeURIComponent(String(input).slice('/api/state/'.length)),
      JSON.parse(String(init?.body)) as unknown,
    ]);
}

/** Installs the fake backend and the starting state of a test. */
export function setUp(state: Record<string, JsonValue>): void {
  vi.stubGlobal('fetch', fetchMock);
  session.state = state;
  fakeBackend();
}

export function tearDown(): void {
  fetchMock.mockReset();
  vi.unstubAllGlobals();
}
