import type { Locator, Page } from '@playwright/test';

import { EXAMPLE_VERSION, type RemoteHub, expect, test } from './app';
import { expectAccessible } from './axe';

/** What the tests set the installed data-explorer back to: older than every release. */
const EARLIER = `${EXAMPLE_VERSION}-e2e.1`;

/** Longest wait for a dashboard to reach a state: its process has to start. */
const STATE_TIMEOUT_MS = 30_000;

/** The card of one dashboard in the hub's list. */
function card(page: Page, name: string): Locator {
  return page
    .getByRole('listitem')
    .filter({ has: page.getByRole('heading', { level: 3, name, exact: true }) });
}

/** Waits until the card of `name` shows `state`. */
async function expectState(page: Page, name: string, state: string): Promise<void> {
  await expect(card(page, name).getByText(state, { exact: true })).toBeVisible({
    timeout: STATE_TIMEOUT_MS,
  });
}

async function start(page: Page, name: string): Promise<void> {
  await card(page, name).getByRole('button', { name: `Start ${name}` }).click();
  await expectState(page, name, 'Running');
}

test.describe('hub', () => {
  // Each test installs both examples and starts dashboard processes, which
  // can take longer than Playwright's default 30 s on a loaded runner.
  test.describe.configure({ timeout: 120_000 });

  test('lists the installed dashboards, stopped', async ({ page, launchHub }) => {
    await launchHub();

    await expect(page.getByRole('list', { name: 'Installed dashboards' })).toBeVisible();
    for (const name of ['Data Explorer', 'Signal Lab']) {
      await expect(card(page, name)).toBeVisible();
      await expectState(page, name, 'Stopped');
      await expect(card(page, name).getByRole('button', { name: `Start ${name}` })).toBeEnabled();
      await expect(card(page, name).getByRole('button', { name: `Open ${name}` })).toBeDisabled();
    }
    await expect(page.getByRole('button', { name: 'Stop all' })).toBeDisabled();
    await expectAccessible(page, 'hub with stopped dashboards');
  });

  test('starts a dashboard, opens it in a new tab, then stops it', async ({ page, launchHub }) => {
    await launchHub();

    await start(page, 'Data Explorer');
    await expect(page.getByRole('status')).toHaveText('Data Explorer is running.');
    await expectAccessible(page, 'hub with a running dashboard');
    // The other dashboard is untouched.
    await expectState(page, 'Signal Lab', 'Stopped');

    const [tab] = await Promise.all([
      page.context().waitForEvent('page'),
      card(page, 'Data Explorer').getByRole('button', { name: 'Open Data Explorer' }).click(),
    ]);
    await expect(tab.getByRole('heading', { level: 1, name: 'Data Explorer' })).toBeVisible();
    await expectAccessible(tab, 'dashboard opened from the hub');
    await tab.close();

    await expectAccessible(page, 'hub after opening a dashboard');
    await card(page, 'Data Explorer').getByRole('button', { name: 'Stop Data Explorer' }).click();
    await expectState(page, 'Data Explorer', 'Stopped');
    await expect(page.getByRole('status')).toHaveText('Data Explorer is stopped.');
    await expectAccessible(page, 'hub after stopping a dashboard');
  });

  test('stops every running dashboard with Stop all', async ({ page, launchHub }) => {
    await launchHub();
    await start(page, 'Data Explorer');
    await start(page, 'Signal Lab');
    const stopAll = page.getByRole('button', { name: 'Stop all' });
    await expect(stopAll).toBeEnabled();
    await expectAccessible(page, 'hub with two running dashboards');

    await stopAll.click();

    await expectState(page, 'Data Explorer', 'Stopped');
    await expectState(page, 'Signal Lab', 'Stopped');
    await expect(stopAll).toBeDisabled();
    await expectAccessible(page, 'hub after Stop all');
  });

  test('uninstalls a stopped dashboard after a confirmation', async ({ page, launchHub }) => {
    await launchHub();
    await start(page, 'Signal Lab');
    // A running dashboard cannot be uninstalled.
    await expect(
      card(page, 'Signal Lab').getByRole('button', { name: 'Uninstall Signal Lab' }),
    ).toBeDisabled();
    await card(page, 'Signal Lab').getByRole('button', { name: 'Stop Signal Lab' }).click();
    await expectState(page, 'Signal Lab', 'Stopped');

    // Declining keeps it.
    page.once('dialog', (dialog) => void dialog.dismiss());
    await card(page, 'Signal Lab').getByRole('button', { name: 'Uninstall Signal Lab' }).click();
    await expect(card(page, 'Signal Lab')).toBeVisible();

    page.once('dialog', (dialog) => void dialog.accept());
    await card(page, 'Signal Lab').getByRole('button', { name: 'Uninstall Signal Lab' }).click();

    await expect(card(page, 'Signal Lab')).toHaveCount(0);
    await expect(page.getByRole('status')).toHaveText('Signal Lab uninstalled.');
    await expect(card(page, 'Data Explorer')).toBeVisible();
    await expectAccessible(page, 'hub after uninstalling a dashboard');
  });
});

/**
 * Longest wait for a preview, an install or an update: a debug build of the
 * hub downloads, hashes and runs an executable of 50 to 110 MB.
 */
const REMOTE_TIMEOUT_MS = 90_000;

/** The confirmation dialog of an install or an update. */
function dialog(page: Page): Locator {
  return page.getByRole('dialog');
}

/** The page's own status region, outside the dialog. */
function pageStatus(page: Page): Locator {
  return page.getByRole('main').getByRole('status');
}

/** Installs data-explorer from the default catalog, whose key signs it. */
async function installFromCatalog(page: Page): Promise<void> {
  await page.getByRole('button', { name: 'Install Data Explorer from lab/data-explorer' }).click();
  await expect(dialog(page).getByRole('heading', { name: 'Install Data Explorer?' })).toBeVisible({
    timeout: REMOTE_TIMEOUT_MS,
  });
  await expect(dialog(page).getByRole('heading', { name: 'Trusted key' })).toBeVisible();
  await expectAccessible(page, 'install dialog, key listed by the catalog');
  await dialog(page).getByRole('button', { name: 'Install', exact: true }).click();
  await expect(dialog(page)).toHaveCount(0, { timeout: REMOTE_TIMEOUT_MS });
  await expect(pageStatus(page)).toHaveText('Data Explorer installed.');
}

/** Sets the installed data-explorer back, changes the release, and checks. */
async function offerUpdate(page: Page, hub: RemoteHub, change: string): Promise<void> {
  await hub.setInstalledVersion('data-explorer', EARLIER);
  await hub.control(change);
  await page.getByRole('button', { name: 'Check now' }).click();
  const available = card(page, 'Data Explorer').getByText(`v${EXAMPLE_VERSION} available`);
  await expect(available).toBeVisible({ timeout: REMOTE_TIMEOUT_MS });
  await expect(pageStatus(page)).toContainText(`Data Explorer v${EXAMPLE_VERSION} is available.`);
}

test.describe('hub with GitHub', () => {
  // Each test downloads and verifies executables of 50 to 110 MB with a
  // debug build of the hub.
  test.describe.configure({ timeout: 240_000 });

  test('installs from a catalog, then updates and rolls back', async ({
    page,
    launchRemoteHub,
  }) => {
    const hub = await launchRemoteHub();
    await expect(page.getByRole('list', { name: 'Dashboards of Example catalog' })).toBeVisible();
    await expect(page.getByText(/No dashboard is installed/)).toBeVisible();
    await expectAccessible(page, 'empty hub with a catalog');

    await installFromCatalog(page);
    const explorer = card(page, 'Data Explorer');
    await expect(explorer.getByText('From GitHub: lab/data-explorer')).toBeVisible();
    await expect(explorer.getByText(`Version ${EXAMPLE_VERSION}`)).toBeVisible();
    await expect(
      page.getByRole('list', { name: 'Dashboards of Example catalog' }).getByText('Installed'),
    ).toBeVisible();
    await expectAccessible(page, 'hub with a dashboard installed from GitHub');

    await offerUpdate(page, hub, 'data-explorer/newer');
    await expectAccessible(page, 'hub with an update available');

    await explorer.getByRole('button', { name: 'Update Data Explorer' }).click();
    await expect(dialog(page).getByRole('heading', { name: 'Update Data Explorer?' })).toBeVisible({
      timeout: REMOTE_TIMEOUT_MS,
    });
    await expect(
      dialog(page).getByText('The same key signed what you installed from this repository.'),
    ).toBeVisible();
    await expectAccessible(page, 'update dialog, pinned key');
    await dialog(page).getByRole('button', { name: 'Update', exact: true }).click();
    await expect(dialog(page)).toHaveCount(0, { timeout: REMOTE_TIMEOUT_MS });
    await expect(pageStatus(page)).toHaveText(
      `Data Explorer updated to version ${EXAMPLE_VERSION}.`,
    );
    await expect(explorer.getByText(`v${EXAMPLE_VERSION} available`)).toHaveCount(0);
    await expectAccessible(page, 'hub after an update');

    page.once('dialog', (confirmation) => void confirmation.accept());
    await explorer.getByRole('button', { name: 'Roll back Data Explorer' }).click();
    await expect(pageStatus(page)).toHaveText(`Data Explorer rolled back to version ${EARLIER}.`, {
      timeout: REMOTE_TIMEOUT_MS,
    });
    await expect(explorer.getByRole('button', { name: 'Roll back Data Explorer' })).toHaveCount(0);
    await expectAccessible(page, 'hub after a rollback');
  });

  test('refuses an update signed with a changed key, and says what could not be checked', async ({
    page,
    launchRemoteHub,
  }) => {
    const hub = await launchRemoteHub();
    await installFromCatalog(page);
    await offerUpdate(page, hub, 'data-explorer/new-key');

    const explorer = card(page, 'Data Explorer');
    const update = explorer.getByRole('button', { name: 'Update Data Explorer' });
    await update.click();
    const alarm = dialog(page).getByRole('alert');
    await expect(alarm).toContainText('Warning: the signing key has changed', {
      timeout: REMOTE_TIMEOUT_MS,
    });
    await expect(alarm).toContainText('Someone else may be publishing in its name.');
    await expect(dialog(page).getByRole('button', { name: 'Update', exact: true })).toHaveCount(0);
    await expect(dialog(page).getByRole('button', { name: 'Trust this key' })).toBeVisible();
    await expectAccessible(page, 'update dialog, changed key');

    await page.keyboard.press('Escape');
    await expect(dialog(page)).toHaveCount(0);
    await expect(pageStatus(page)).toHaveText('Nothing was updated.');
    await expect(explorer.getByText(`Version ${EARLIER}`)).toBeVisible();
    await expect(update).toBeFocused();
    await expectAccessible(page, 'hub after a refused key change');

    await hub.control('data-explorer/offline');
    await page.getByRole('button', { name: 'Check now' }).click();
    await expect(pageStatus(page)).toContainText('Data Explorer could not be checked.', {
      timeout: REMOTE_TIMEOUT_MS,
    });
    const problems = page.getByRole('list', { name: 'Could not check' });
    await expect(problems).toContainText('Data Explorer could not be checked:');
    await expect(problems).toContainText('(github_rate_limited)');
    await expectAccessible(page, 'hub after a check that could not reach GitHub');
  });

  test('installs from a repository with an unknown key once the user trusts it', async ({
    page,
    launchRemoteHub,
  }) => {
    await launchRemoteHub();
    await page.getByLabel('Repository (owner/repo)', { exact: true }).fill('lab/signal-lab');
    await page.getByRole('button', { name: 'Add repository' }).click();

    await expect(dialog(page).getByRole('heading', { name: 'Unknown key' })).toBeVisible({
      timeout: REMOTE_TIMEOUT_MS,
    });
    await expect(
      dialog(page).getByRole('heading', { name: 'Install lab/signal-lab?' }),
    ).toBeVisible();
    await expect(
      dialog(page).getByText('Read from the executable once you trust its key'),
    ).toBeVisible();
    await expect(dialog(page).getByRole('button', { name: 'Install', exact: true })).toHaveCount(0);
    await expectAccessible(page, 'install dialog, unknown key');

    await dialog(page).getByRole('button', { name: 'Trust this key' }).click();
    await expect(dialog(page).getByRole('heading', { name: 'Key trusted by you' })).toBeVisible({
      timeout: REMOTE_TIMEOUT_MS,
    });
    await expect(dialog(page).getByRole('heading', { name: 'Install Signal Lab?' })).toBeVisible();
    await expect(
      dialog(page).getByText(`Signal Lab, version ${EXAMPLE_VERSION}`, { exact: true }),
    ).toBeVisible();
    await expect(dialog(page).getByRole('status')).toHaveText(
      `Key trusted. Signal Lab, version ${EXAMPLE_VERSION}.`,
    );
    // Not lost with the button it was on, and not on the one that installs.
    await expect(dialog(page).getByRole('button', { name: 'Cancel' })).toBeFocused();
    await expectAccessible(page, 'install dialog, key trusted by the user');

    await dialog(page).getByRole('button', { name: 'Install', exact: true }).click();
    await expect(dialog(page)).toHaveCount(0, { timeout: REMOTE_TIMEOUT_MS });
    await expect(pageStatus(page)).toHaveText('Signal Lab installed.');
    await expect(card(page, 'Signal Lab').getByText('From GitHub: lab/signal-lab')).toBeVisible();
    await expectAccessible(page, 'hub after an install from a repository');
  });

  test('installs two dashboards of one repository, by their slugs', async ({
    page,
    launchRemoteHub,
  }) => {
    await launchRemoteHub();
    // lab/suite publishes signal-lab and data-explorer in one release.
    await page.getByLabel('Repository (owner/repo)', { exact: true }).fill('lab/suite');
    await page.getByRole('button', { name: 'Add repository' }).click();
    const refusal = page.getByRole('alert');
    await expect(refusal).toContainText('choose the one to install by its slug', {
      timeout: REMOTE_TIMEOUT_MS,
    });
    await expect(refusal).toContainText('data-explorer, signal-lab');
    await expect(refusal).toContainText('(hub_ambiguous_asset)');
    const slug = page.getByLabel('Dashboard slug (optional)');
    await expect(slug).toBeFocused();
    await expect(dialog(page)).toHaveCount(0);
    await expectAccessible(page, 'a repository that publishes several dashboards, no slug given');

    // Named by its slug in the form: no catalog lists the repository yet.
    await slug.fill('signal-lab');
    await page.getByRole('button', { name: 'Add repository' }).click();
    await expect(dialog(page).getByRole('heading', { name: 'Unknown key' })).toBeVisible({
      timeout: REMOTE_TIMEOUT_MS,
    });
    await dialog(page).getByRole('button', { name: 'Trust this key' }).click();
    await expect(dialog(page).getByRole('heading', { name: 'Install Signal Lab?' })).toBeVisible({
      timeout: REMOTE_TIMEOUT_MS,
    });
    await dialog(page).getByRole('button', { name: 'Install', exact: true }).click();
    await expect(dialog(page)).toHaveCount(0, { timeout: REMOTE_TIMEOUT_MS });
    await expect(pageStatus(page)).toHaveText('Signal Lab installed.');
    await expect(card(page, 'Signal Lab').getByText('From GitHub: lab/suite')).toBeVisible();

    // A catalog that lists the repository once per slug.
    await page.getByLabel('Catalog repository (owner/repo)').fill('lab/suite-catalog');
    await page.getByRole('button', { name: 'Add catalog' }).click();
    await expect(pageStatus(page)).toHaveText('Catalog lab/suite-catalog added.');
    const suite = page.getByRole('list', { name: 'Dashboards of Suite catalog' });
    await expect(suite.getByText('lab/suite, dashboard signal-lab')).toBeVisible();
    await expect(suite.getByText('Installed')).toHaveCount(1);
    await expectAccessible(page, 'a catalog that lists one repository once per slug');

    // The second dashboard of the repository: its key is the one pinned.
    await suite.getByRole('button', { name: 'Install Data Explorer from lab/suite' }).click();
    await expect(dialog(page).getByRole('heading', { name: 'Install Data Explorer?' })).toBeVisible({
      timeout: REMOTE_TIMEOUT_MS,
    });
    await expect(
      dialog(page).getByText('The same key signed what you installed from this repository.'),
    ).toBeVisible();
    await expectAccessible(page, 'install dialog, second dashboard of a repository');
    await dialog(page).getByRole('button', { name: 'Install', exact: true }).click();
    await expect(dialog(page)).toHaveCount(0, { timeout: REMOTE_TIMEOUT_MS });
    await expect(pageStatus(page)).toHaveText('Data Explorer installed.');
    await expect(card(page, 'Data Explorer').getByText('From GitHub: lab/suite')).toBeVisible();
    await expect(card(page, 'Signal Lab').getByText('From GitHub: lab/suite')).toBeVisible();
    await expect(suite.getByText('Installed')).toHaveCount(2);
    await expectAccessible(page, 'two dashboards installed from one repository');
  });

  test('adds and removes catalogs, and turns the check at start off', async ({
    page,
    launchRemoteHub,
  }) => {
    await launchRemoteHub();
    const input = page.getByLabel('Catalog repository (owner/repo)');
    await input.fill('not a repository');
    await page.getByRole('button', { name: 'Add catalog' }).click();
    await expect(page.getByRole('alert')).toContainText('Enter a repository as owner/repo');
    await expect(input).toHaveAttribute('aria-invalid', 'true');
    await expectAccessible(page, 'catalog repository refused before any request');

    await input.fill('lab/catalog');
    await page.getByRole('button', { name: 'Add catalog' }).click();
    await expect(pageStatus(page)).toHaveText('Catalog lab/catalog added.');
    await expect(
      page.getByRole('button', { name: 'Install Signal Lab from lab/signal-lab' }),
    ).toBeVisible();
    await expectAccessible(page, 'hub with two catalogs');

    page.once('dialog', (confirmation) => void confirmation.accept());
    await page.getByRole('button', { name: 'Remove catalog rtonneau/eario-catalog' }).click();
    await expect(pageStatus(page)).toHaveText('Catalog rtonneau/eario-catalog removed.');
    await expect(page.getByRole('list', { name: 'Dashboards of Example catalog' })).toHaveCount(0);

    const toggle = page.getByLabel('Check for updates at start');
    await expect(toggle).toBeChecked();
    await toggle.uncheck();
    await expect(pageStatus(page)).toHaveText('The hub will not check for updates when it starts.');
    await expectAccessible(page, 'hub with the check at start off');

    // Both are stored by the hub, not by the page.
    await page.reload();
    await expect(page.getByLabel('Check for updates at start')).not.toBeChecked();
    await expect(page.getByRole('list', { name: 'Dashboards of Lab catalog' })).toBeVisible();
    await expect(page.getByRole('list', { name: 'Dashboards of Example catalog' })).toHaveCount(0);
  });
});
