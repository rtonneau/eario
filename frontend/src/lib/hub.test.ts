import { describe, expect, it } from 'vitest';

import type { HubDashboard } from './generated/HubDashboard';
import type { HubInfo } from './generated/HubInfo';
import type { KeyStatus } from './generated/KeyStatus';
import type { UpdateCheck } from './generated/UpdateCheck';
import {
  checkProblems,
  checkSummary,
  formatCheckedAt,
  hubUpdatesItself,
  installedFrom,
  keyExplanation,
  looksLikeRepo,
  sourceText,
  versionLabel,
} from './hub';

function dashboard(slug: string, patch: Partial<HubDashboard> = {}): HubDashboard {
  return {
    slug,
    name: slug.toUpperCase(),
    version: '1.0.0',
    description: '',
    state: 'stopped',
    log_lines: [],
    source: { kind: 'local' },
    available_version: null,
    has_previous: false,
    ...patch,
  };
}

/** 27 November 2026, 12:00 UTC. */
const CHECKED_AT = 1_795_780_800;

function check(patch: Partial<UpdateCheck> = {}): UpdateCheck {
  return { checked_at: CHECKED_AT, dashboards: [], hub: null, ...patch };
}

function info(patch: Partial<HubInfo> = {}): HubInfo {
  return {
    eario_version: '0.2.0',
    dashboards: [],
    last_check: null,
    available_version: null,
    ...patch,
  };
}

describe('looksLikeRepo', () => {
  it('takes owner/repo as GitHub names them', () => {
    for (const repo of ['lab/catalog', 'a/b', 'Lab-2/my.repo_name-1', `${'o'.repeat(39)}/r`]) {
      expect(looksLikeRepo(repo), repo).toBe(true);
    }
  });

  it('refuses anything else', () => {
    for (const repo of [
      '',
      'lab',
      'lab/',
      '/catalog',
      'lab/catalog/extra',
      'lab/..',
      'lab/.',
      'la b/catalog',
      'lab/cat alog',
      'lab_x/catalog',
      'https://github.com/lab/catalog',
      `${'o'.repeat(40)}/r`,
      `o/${'r'.repeat(101)}`,
    ]) {
      expect(looksLikeRepo(repo), repo).toBe(false);
    }
  });
});

describe('installedFrom', () => {
  it('finds the dashboard installed from a repository, whatever the case', () => {
    const fromGitHub = dashboard('explorer', { source: { kind: 'github', repo: 'Lab/Explorer' } });
    const dashboards = [dashboard('local'), fromGitHub];
    expect(installedFrom('lab/explorer', dashboards)).toBe(fromGitHub);
    expect(installedFrom('lab/other', dashboards)).toBeUndefined();
  });

  it('never takes a local dashboard of the same slug for the repository', () => {
    expect(installedFrom('lab/local', [dashboard('local')])).toBeUndefined();
  });
});

describe('sourceText and versionLabel', () => {
  it('names where a dashboard came from', () => {
    expect(sourceText(dashboard('a'))).toBe('Installed from a local executable');
    expect(sourceText(dashboard('a', { source: { kind: 'github', repo: 'lab/a' } }))).toBe(
      'From GitHub: lab/a',
    );
  });

  it('prefixes a version with v once', () => {
    expect(versionLabel('1.2.0')).toBe('v1.2.0');
    expect(versionLabel('v1.2.0')).toBe('v1.2.0');
  });
});

describe('keyExplanation', () => {
  it('says which keys are trusted and which need the user', () => {
    const levels = (['catalog', 'pinned', 'accepted', 'unknown', 'changed'] as KeyStatus[]).map(
      (status) => keyExplanation(status).level,
    );
    expect(levels).toEqual(['trusted', 'trusted', 'trusted', 'warning', 'danger']);
  });

  it('tells the user to compare an unknown key, and warns loudly about a changed one', () => {
    expect(keyExplanation('unknown').text).toMatch(/Compare the key/);
    const changed = keyExplanation('changed');
    expect(changed.title).toMatch(/^Warning/);
    expect(changed.text).toMatch(/Someone else may be publishing/);
  });
});

describe('update checks', () => {
  it('formats the time of a check', () => {
    expect(formatCheckedAt(CHECKED_AT)).toMatch(/Nov 2[67], 2026/);
  });

  it('lists what could not be checked, with message and code', () => {
    const problems = checkProblems(
      check({
        dashboards: [
          {
            slug: 'explorer',
            repo: 'lab/explorer',
            latest_version: null,
            available_version: null,
            error: { code: 'github_rate_limited', message: 'GitHub limits requests.' },
          },
          {
            slug: 'gone',
            repo: 'lab/gone',
            latest_version: '1.0.0',
            available_version: null,
            error: null,
          },
        ],
        hub: {
          repo: 'rtonneau/eario',
          latest_version: null,
          available_version: null,
          error: { code: 'github_offline', message: 'No network.' },
        },
      }),
      [dashboard('explorer', { name: 'Explorer' })],
    );
    expect(problems).toEqual([
      'Explorer could not be checked: GitHub limits requests. (github_rate_limited)',
      'The hub could not be checked: No network. (github_offline)',
    ]);
  });

  it('does not count a hub that does not update itself as a problem', () => {
    const notConfigured = check({
      hub: {
        repo: 'rtonneau/eario',
        latest_version: null,
        available_version: null,
        error: { code: 'self_update_not_configured', message: 'No key.' },
      },
    });
    expect(checkProblems(notConfigured, [])).toEqual([]);
    expect(hubUpdatesItself(notConfigured)).toBe(false);
    expect(hubUpdatesItself(check())).toBe(true);
    expect(hubUpdatesItself(null)).toBe(true);
  });

  it('sums a check up in one announcement', () => {
    expect(checkSummary(info())).toBe('Not checked for updates yet.');
    expect(checkSummary(info({ last_check: check() }))).toMatch(
      /^Checked for updates at .+\. No update available\.$/,
    );
    const summary = checkSummary(
      info({
        dashboards: [
          dashboard('explorer', { name: 'Explorer', available_version: '2.0.0' }),
          dashboard('lab', { name: 'Lab' }),
        ],
        available_version: '0.3.0',
        last_check: check({
          dashboards: [
            {
              slug: 'lab',
              repo: 'lab/lab',
              latest_version: null,
              available_version: null,
              error: { code: 'github_offline', message: 'No network.' },
            },
          ],
        }),
      }),
    );
    expect(summary).toMatch(
      / Explorer v2\.0\.0 is available\. Eario hub v0\.3\.0 is available\. Lab could not be checked\.$/,
    );
  });
});
