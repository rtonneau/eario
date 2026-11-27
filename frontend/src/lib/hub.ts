// What the hub's page says about repositories, signing keys, sources and
// update checks. Pure functions of the contract types; the hub decides
// everything, these only put its answers into words.

import type { HubCatalogEntry } from './generated/HubCatalogEntry';
import type { HubDashboard } from './generated/HubDashboard';
import type { HubInfo } from './generated/HubInfo';
import type { KeyStatus } from './generated/KeyStatus';
import type { UpdateCheck } from './generated/UpdateCheck';

/** Code of a hub build that holds no Eario release key: it does not update itself. */
export const SELF_UPDATE_NOT_CONFIGURED = 'self_update_not_configured';

const OWNER = /^[A-Za-z0-9-]{1,39}$/;
const NAME = /^[A-Za-z0-9._-]{1,100}$/;

/**
 * Whether `text` has the shape of a GitHub repository, `owner/repo`. Only a
 * hint for the user before a request: the hub checks every repository again.
 */
export function looksLikeRepo(text: string): boolean {
  const parts = text.split('/');
  if (parts.length !== 2) {
    return false;
  }
  const [owner = '', name = ''] = parts;
  return OWNER.test(owner) && NAME.test(name) && name !== '.' && name !== '..';
}

const SLUG = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

/**
 * Whether `text` has the shape of a dashboard's slug: lower-case letters and
 * digits, in words joined by single dashes. Only a hint before a request:
 * the hub checks every slug again.
 */
export function looksLikeSlug(text: string): boolean {
  return SLUG.test(text);
}

/** Whether `dashboard` was installed from `repo`, compared without case as GitHub does. */
function isFrom(dashboard: HubDashboard, repo: string): boolean {
  return (
    dashboard.source.kind === 'github' &&
    dashboard.source.repo.toLowerCase() === repo.toLowerCase()
  );
}

/** The first dashboard installed from `repo`, compared without case as GitHub does. */
export function installedFrom(
  repo: string,
  dashboards: readonly HubDashboard[],
): HubDashboard | undefined {
  return dashboards.find((dashboard) => isFrom(dashboard, repo));
}

/**
 * The dashboard installed from a catalog entry: its slug from its
 * repository, or, for an entry that names no slug, the first dashboard
 * installed from the repository.
 */
export function installedFromEntry(
  entry: Pick<HubCatalogEntry, 'repo' | 'slug'>,
  dashboards: readonly HubDashboard[],
): HubDashboard | undefined {
  if (entry.slug === null) {
    return installedFrom(entry.repo, dashboards);
  }
  return dashboards.find(
    (dashboard) => dashboard.slug === entry.slug && isFrom(dashboard, entry.repo),
  );
}

/** Where a dashboard was installed from, in words. */
export function sourceText(dashboard: HubDashboard): string {
  return dashboard.source.kind === 'github'
    ? `From GitHub: ${dashboard.source.repo}`
    : 'Installed from a local executable';
}

/** A version as shown on a badge: `v1.2.0`. */
export function versionLabel(version: string): string {
  return version.startsWith('v') ? version : `v${version}`;
}

/** What a key status means for the user, and how loudly to say it. */
export interface KeyExplanation {
  /** Short title. */
  title: string;
  /** What it means and what to do, in plain words. */
  text: string;
  /** `trusted`: nothing to do; `warning`: check the key; `danger`: likely an attack. */
  level: 'trusted' | 'warning' | 'danger';
}

/** Explains a key status of an install preview. */
export function keyExplanation(status: KeyStatus): KeyExplanation {
  switch (status) {
    case 'catalog':
      return {
        title: 'Trusted key',
        text: 'A catalog you use lists this key for this repository.',
        level: 'trusted',
      };
    case 'pinned':
      return {
        title: 'Trusted key',
        text: 'The same key signed what you installed from this repository.',
        level: 'trusted',
      };
    case 'accepted':
      return {
        title: 'Key trusted by you',
        text: 'You chose to trust this key. It is kept with the dashboard once it is installed.',
        level: 'trusted',
      };
    case 'unknown':
      return {
        title: 'Unknown key',
        text:
          'No catalog you use lists this repository, so the hub cannot tell whose key this is. ' +
          'Compare the key above with the one the author publishes, on a page you trust, ' +
          'before you trust it. The download matches the key, but nothing from it has run yet.',
        level: 'warning',
      };
    case 'changed':
      return {
        title: 'Warning: the signing key has changed',
        text:
          'This release is signed with a different key from the one the hub knows for this ' +
          'repository. Someone else may be publishing in its name. Do not trust the new key ' +
          'unless its author told you, on a page you trust, that the key changed. Nothing ' +
          'from this release has run.',
        level: 'danger',
      };
  }
}

/** Formats the time of a check, given in seconds since the Unix epoch. */
export function formatCheckedAt(seconds: number): string {
  return new Intl.DateTimeFormat('en-US', { dateStyle: 'medium', timeStyle: 'short' }).format(
    new Date(seconds * 1000),
  );
}

/** Name of the dashboard `slug` in `info`, or the slug once it is gone. */
function nameOf(slug: string, dashboards: readonly HubDashboard[]): string {
  return dashboards.find((dashboard) => dashboard.slug === slug)?.name ?? slug;
}

/** What could not be checked, one sentence each, with the hub's message and code. */
export function checkProblems(check: UpdateCheck, dashboards: readonly HubDashboard[]): string[] {
  const problems = check.dashboards.flatMap((entry) =>
    entry.error === null
      ? []
      : [
          `${nameOf(entry.slug, dashboards)} could not be checked: ` +
            `${entry.error.message} (${entry.error.code})`,
        ],
  );
  const hubError = check.hub?.error;
  if (hubError && hubError.code !== SELF_UPDATE_NOT_CONFIGURED) {
    problems.push(`The hub could not be checked: ${hubError.message} (${hubError.code})`);
  }
  return problems;
}

/** Whether this build of the hub updates itself, as far as the last check tells. */
export function hubUpdatesItself(check: UpdateCheck | null): boolean {
  return check?.hub?.error?.code !== SELF_UPDATE_NOT_CONFIGURED;
}

/** One sentence on the last check, for the status region. */
export function checkSummary(info: HubInfo): string {
  const check = info.last_check;
  if (check === null) {
    return 'Not checked for updates yet.';
  }
  const updates = info.dashboards.filter((dashboard) => dashboard.available_version !== null);
  const parts = [`Checked for updates at ${formatCheckedAt(check.checked_at)}.`];
  if (updates.length === 0 && info.available_version === null) {
    parts.push('No update available.');
  }
  for (const dashboard of updates) {
    parts.push(`${dashboard.name} ${versionLabel(dashboard.available_version ?? '')} is available.`);
  }
  if (info.available_version !== null) {
    parts.push(`Eario hub ${versionLabel(info.available_version)} is available.`);
  }
  const failed = check.dashboards.filter((entry) => entry.error !== null);
  for (const entry of failed) {
    parts.push(`${nameOf(entry.slug, info.dashboards)} could not be checked.`);
  }
  const hubError = check.hub?.error;
  if (hubError && hubError.code !== SELF_UPDATE_NOT_CONFIGURED) {
    parts.push('The hub could not be checked.');
  }
  return parts.join(' ');
}
