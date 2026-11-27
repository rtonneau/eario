// Communication with the Rust backend. This is the only module that calls
// fetch. The types come from `generated/`, written by `cargo xtask gen-types`
// from the Rust contract types.

import type { ApiError } from './generated/ApiError';
import type { AppResponse } from './generated/AppResponse';
import type { ComponentData } from './generated/ComponentData';
import type { HubCatalog } from './generated/HubCatalog';
import type { HubInfo } from './generated/HubInfo';
import type { HubOpen } from './generated/HubOpen';
import type { HubSelfUpdate } from './generated/HubSelfUpdate';
import type { HubSettings } from './generated/HubSettings';
import type { InstallPreview } from './generated/InstallPreview';
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

/** Whether `name` would not stay a path segment of its own. */
function isDotOrEmpty(name: string): boolean {
  return name === '' || name === '.' || name === '..';
}

/**
 * `name` as one path segment of a route, percent-encoded (`/` included).
 *
 * Throws, before any request, for an empty name, `.` or `..`: encoding
 * leaves those as they are, and the URL parser drops or climbs on them, so
 * the request would reach another route.
 */
function segment(name: string): string {
  if (isDotOrEmpty(name)) {
    throw new ApiFailure(
      `The name "${name}" cannot be sent to the application: a name must not be empty, "." or "..".`,
      'invalid_path_segment',
      0,
    );
  }
  return encodeURIComponent(name);
}

/**
 * A GitHub repository `owner/repo` as the two path segments the hub's routes
 * take. Throws, before any request, when the owner or the repository name is
 * empty, `.` or `..`. A further `/` is encoded into the repository name,
 * which the hub then refuses.
 */
function repository(repo: string): string {
  const slash = repo.indexOf('/');
  const owner = slash === -1 ? repo : repo.slice(0, slash);
  const name = slash === -1 ? '' : repo.slice(slash + 1);
  if (isDotOrEmpty(owner) || isDotOrEmpty(name)) {
    throw new ApiFailure(
      `"${repo}" is not a repository of the form owner/repo.`,
      'invalid_path_segment',
      0,
    );
  }
  return `${segment(owner)}/${segment(name)}`;
}

/**
 * The route prefix of a release of `repo`: of the dashboard `slug` among
 * those the repository publishes, or, with `null`, of its only one.
 */
function source(repo: string, slug: string | null): string {
  const base = `/sources/${repository(repo)}`;
  return slug === null ? base : `${base}/dashboards/${segment(slug)}`;
}

/**
 * The backend's routes. A name that cannot be a path segment (see
 * `segment`) throws an `ApiFailure` before any request is sent.
 */
export const api = {
  app: () => request<AppResponse>('GET', '/app'),

  setState: (key: string, value: JsonValue) =>
    request<StateUpdate>('PUT', `/state/${segment(key)}`, JSON.stringify(value), true),

  componentData: (id: string, offset = 0) =>
    request<ComponentData>('GET', `/components/${segment(id)}/data?offset=${offset}`),

  upload: (dataset: string, file: File) =>
    request<StateUpdate>(
      'POST',
      // A query value, not a path segment: any file name is sent.
      `/datasets/${segment(dataset)}/upload?filename=${encodeURIComponent(file.name)}`,
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

  // The hub's routes. Every parameter is a path segment the hub checks: a
  // slug, a repository, a SHA-256, or on/off.
  hubStart: (slug: string) => request<HubInfo>('POST', `/dashboards/${segment(slug)}/start`),

  hubStop: (slug: string) => request<HubInfo>('POST', `/dashboards/${segment(slug)}/stop`),

  hubOpen: (slug: string) => request<HubOpen>('POST', `/dashboards/${segment(slug)}/open`),

  hubUninstall: (slug: string) =>
    request<HubInfo>('POST', `/dashboards/${segment(slug)}/uninstall`),

  hubStopAll: () => request<HubInfo>('POST', '/stop-all'),

  hubCatalogs: () => request<HubCatalog[]>('GET', '/catalogs'),

  hubAddCatalog: (repo: string) =>
    request<HubCatalog[]>('POST', `/catalogs/${repository(repo)}/add`),

  hubRemoveCatalog: (repo: string) =>
    request<HubCatalog[]>('POST', `/catalogs/${repository(repo)}/remove`),

  // A preview names the dashboard when the repository publishes several;
  // accepting its key and installing it always name the previewed slug, so
  // no other preview of the repository is ever the one confirmed.
  hubPreview: (repo: string, slug: string | null = null) =>
    request<InstallPreview>('POST', `${source(repo, slug)}/preview`),

  hubAcceptKey: (repo: string, slug: string | null, keySha256: string) =>
    request<InstallPreview>('POST', `${source(repo, slug)}/accept-key/${segment(keySha256)}`),

  hubInstall: (repo: string, slug: string | null, sha256: string) =>
    request<HubInfo>('POST', `${source(repo, slug)}/install/${segment(sha256)}`),

  hubCheck: () => request<HubInfo>('POST', '/check'),

  hubUpdatePreview: (slug: string) =>
    request<InstallPreview>('POST', `/dashboards/${segment(slug)}/update-preview`),

  hubUpdate: (slug: string, sha256: string) =>
    request<HubInfo>('POST', `/dashboards/${segment(slug)}/update/${segment(sha256)}`),

  hubRollback: (slug: string) => request<HubInfo>('POST', `/dashboards/${segment(slug)}/rollback`),

  hubSettings: () => request<HubSettings>('GET', '/settings'),

  hubSetCheckAtStart: (on: boolean) =>
    request<HubSettings>('POST', `/settings/check-at-start/${on ? 'on' : 'off'}`),

  hubSelfUpdate: () => request<HubSelfUpdate>('POST', '/self-update'),
};

/** Message to show for any thrown value. */
export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Message to show for any thrown value, followed by its code when it has one. */
export function errorWithCode(error: unknown): string {
  return error instanceof ApiFailure
    ? `${error.message} (${error.code})`
    : errorMessage(error);
}
