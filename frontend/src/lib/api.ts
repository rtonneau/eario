// Communication with the Rust backend. This is the only module that calls
// fetch. The types come from `generated/`, written by `cargo xtask gen-types`
// from the Rust contract types.

import type { ApiError } from './generated/ApiError';
import type { AppResponse } from './generated/AppResponse';
import type { ComponentData } from './generated/ComponentData';
import type { Preferences } from './generated/Preferences';
import type { StateUpdate } from './generated/StateUpdate';
import type { JsonValue } from './generated/serde_json/JsonValue';

const TOKEN_HEADER = 'x-eario-token';
const TOKEN_STORAGE_KEY = 'eario-token';

let token = '';

/**
 * Reads the session token the backend put in the URL fragment, keeps it for
 * the lifetime of the tab, and removes it from the address bar.
 *
 * Returns false when no token is available: the page was opened without the
 * address printed by the application.
 */
export function initToken(): boolean {
  const fragment = new URLSearchParams(window.location.hash.slice(1));
  const fromUrl = fragment.get('token');
  if (fromUrl) {
    window.sessionStorage.setItem(TOKEN_STORAGE_KEY, fromUrl);
    window.history.replaceState(null, '', window.location.pathname + window.location.search);
  }
  token = fromUrl ?? window.sessionStorage.getItem(TOKEN_STORAGE_KEY) ?? '';
  return token !== '';
}

/** A request the backend refused or that never reached it. */
export class ApiFailure extends Error {
  constructor(
    message: string,
    /** Stable code from the backend, or a local one such as `unreachable`. */
    readonly code: string,
    /** HTTP status, or 0 when the backend did not answer. */
    readonly status: number,
  ) {
    super(message);
    this.name = 'ApiFailure';
  }
}

function isApiError(value: unknown): value is ApiError {
  return (
    typeof value === 'object' &&
    value !== null &&
    typeof (value as ApiError).code === 'string' &&
    typeof (value as ApiError).message === 'string'
  );
}

async function failure(response: Response): Promise<ApiFailure> {
  const body: unknown = await response.json().catch(() => null);
  if (isApiError(body)) {
    return new ApiFailure(body.message, body.code, response.status);
  }
  if (response.status === 413) {
    return new ApiFailure(
      'The file is larger than this application accepts.',
      'too_large',
      response.status,
    );
  }
  return new ApiFailure(
    `The application answered with an error (${response.status}).`,
    'http_error',
    response.status,
  );
}

async function request<T>(method: string, path: string, body?: BodyInit, json = false): Promise<T> {
  const headers: Record<string, string> = { [TOKEN_HEADER]: token };
  if (json) {
    headers['content-type'] = 'application/json';
  }
  let response: Response;
  try {
    response = await fetch(`/api${path}`, { method, headers, body });
  } catch {
    throw new ApiFailure(
      'The application is not responding. It may have been stopped.',
      'unreachable',
      0,
    );
  }
  if (!response.ok) {
    throw await failure(response);
  }
  if (response.status === 204) {
    return undefined as T;
  }
  return (await response.json()) as T;
}

const segment = encodeURIComponent;

export const api = {
  app: () => request<AppResponse>('GET', '/app'),

  setState: (key: string, value: JsonValue) =>
    request<StateUpdate>('PUT', `/state/${segment(key)}`, JSON.stringify(value), true),

  componentData: (id: string, offset = 0) =>
    request<ComponentData>('GET', `/components/${segment(id)}/data?offset=${offset}`),

  upload: (dataset: string, file: File) =>
    request<StateUpdate>(
      'POST',
      `/datasets/${segment(dataset)}/upload?filename=${segment(file.name)}`,
      file,
    ),

  selectSheet: (dataset: string, sheet: string) =>
    request<StateUpdate>(
      'PUT',
      `/datasets/${segment(dataset)}/sheet`,
      JSON.stringify({ sheet }),
      true,
    ),

  setPreferences: (preferences: Preferences) =>
    request<Preferences>('PUT', '/preferences', JSON.stringify(preferences), true),

  shutdown: () => request<void>('POST', '/shutdown'),
};

/** Message to show for any thrown value. */
export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
