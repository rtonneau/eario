import { type ChildProcess, execFile, spawn } from 'node:child_process';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';

import { test as base, expect } from '@playwright/test';

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');

/** Directory of the example executables; `cargo xtask e2e` builds them there. */
const binDir = process.env.EARIO_E2E_BIN_DIR ?? path.join(repositoryRoot, 'target', 'debug');

/**
 * Directory the hub tests make their temporary hub directories in;
 * `cargo xtask e2e` empties it before the run. The system's temporary
 * directory when the tests run without it.
 */
const hubRoot = process.env.EARIO_E2E_HUB_ROOT ?? os.tmpdir();

/** Longest wait for an example to print its address. */
const START_TIMEOUT_MS = 30_000;

/** A fixture file, by its path under `fixtures/`. */
export function fixture(relative: string): string {
  return path.join(repositoryRoot, 'fixtures', relative);
}

/** A running example: its address (with the session token) and its process. */
export interface RunningApp {
  url: string;
  process: ChildProcess;
}

/** Path of the executable `name` built by `cargo xtask e2e`. */
function executablePath(name: string): string {
  return path.join(binDir, process.platform === 'win32' ? `${name}.exe` : name);
}

/**
 * Starts the executable `name` with `args`, on a port chosen by the system and
 * without opening a browser, and resolves with the address it prints.
 */
function startApp(
  name: string,
  args: string[],
  env: Record<string, string> = {},
): Promise<RunningApp> {
  const executable = executablePath(name);
  const child = spawn(executable, args, {
    cwd: repositoryRoot,
    env: { ...process.env, EARIO_NO_BROWSER: '1', ...env },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  return new Promise((resolve, reject) => {
    let output = '';
    const timer = setTimeout(() => {
      child.kill();
      reject(new Error(`${name} printed no address within ${START_TIMEOUT_MS} ms:\n${output}`));
    }, START_TIMEOUT_MS);
    const onData = (chunk: Buffer) => {
      output += chunk.toString();
      const match = /is running at (http:\/\/127\.0\.0\.1:\d+\/#token=[0-9a-f]+)/.exec(output);
      const url = match?.[1];
      if (url !== undefined) {
        clearTimeout(timer);
        resolve({ url, process: child });
      }
    };
    child.stdout.on('data', onData);
    child.stderr.on('data', onData);
    child.on('error', (error) => {
      clearTimeout(timer);
      reject(new Error(`cannot start ${executable}: ${error.message}. Run \`cargo xtask e2e\`.`));
    });
    child.on('exit', (code) => {
      clearTimeout(timer);
      reject(new Error(`${name} exited with code ${code} before listening:\n${output}`));
    });
  });
}

/** The hub's session token, from its address. */
function tokenOf(url: string): string {
  return new URLSearchParams(new URL(url).hash.slice(1)).get('token') ?? '';
}

/** Posts to the hub's API as the page does, for the clean-up of a test. */
async function hubPost(url: string, route: string): Promise<void> {
  const origin = new URL(url).origin;
  const response = await fetch(`${origin}/api${route}`, {
    method: 'POST',
    headers: { 'x-eario-token': tokenOf(url) },
  });
  if (!response.ok) {
    throw new Error(`POST ${route} answered ${response.status}`);
  }
}

/** Resolves when `child` has exited, or false after `milliseconds`. */
function exited(child: ChildProcess, milliseconds: number): Promise<boolean> {
  if (child.exitCode !== null || child.signalCode !== null) {
    return Promise.resolve(true);
  }
  return new Promise((resolve) => {
    const timer = setTimeout(() => resolve(false), milliseconds);
    child.once('exit', () => {
      clearTimeout(timer);
      resolve(true);
    });
  });
}

/**
 * Ends the processes recorded in the run files of a hub directory: the last
 * resort when the hub could not stop them. A run file's pid is only for
 * display and could name another process once its own has ended; the risk is
 * accepted here because the directory is this test's own and seconds old.
 */
async function killDashboards(hubDir: string): Promise<void> {
  const runDir = path.join(hubDir, 'run');
  const files = await fs.readdir(runDir).catch(() => [] as string[]);
  for (const file of files) {
    try {
      const run = JSON.parse(await fs.readFile(path.join(runDir, file), 'utf8')) as { pid?: number };
      if (typeof run.pid === 'number') {
        process.kill(run.pid);
      }
    } catch (error) {
      // A dashboard that already ended leaves nothing to kill.
      console.warn(`clean-up: ${file}: ${String(error)}`);
    }
  }
}

/**
 * Stops everything a hub test started, pass or fail: every dashboard through
 * the hub, then the hub itself; whatever does not answer is killed.
 */
async function stopHub(app: RunningApp, hubDir: string): Promise<void> {
  try {
    await hubPost(app.url, '/stop-all');
    await hubPost(app.url, '/shutdown');
  } catch (error) {
    console.warn(`clean-up: the hub did not take stop-all and shutdown: ${String(error)}`);
  }
  if (!(await exited(app.process, 15_000))) {
    app.process.kill();
    await exited(app.process, 5_000);
  }
  // Dashboards are only left when stop-all failed.
  await killDashboards(hubDir);
  // A process that has just ended may still hold its executable on Windows.
  await fs.rm(hubDir, { recursive: true, force: true, maxRetries: 20, retryDelay: 250 });
}

const run = promisify(execFile);

/** Installs the example `name` into `hubDir`, as a user would from a terminal. */
async function installExample(name: string, hubDir: string): Promise<void> {
  await run(executablePath(name), ['--eario-install', '--yes'], {
    cwd: repositoryRoot,
    env: { ...process.env, EARIO_HUB_DIR: hubDir },
  });
}

/** Examples every hub test installs, so each test can start and stop either. */
const INSTALLED_EXAMPLES = ['data-explorer', 'signal-lab'];

/**
 * Tests get `launch`, which starts an example and opens it in the page, and
 * `launchHub`, which installs both examples into a temporary hub directory,
 * starts a hub on it and opens it in the page.
 */
export const test = base.extend<{
  launch: (name: string, args?: string[]) => Promise<void>;
  launchHub: () => Promise<void>;
}>({
  launchHub: async ({ page }, use) => {
    const made: { hubDir?: string; app?: RunningApp } = {};
    try {
      await use(async () => {
        const hubDir = await fs.mkdtemp(path.join(hubRoot, 'hub-'));
        made.hubDir = hubDir;
        // One after the other: each install rewrites the registry.
        for (const name of INSTALLED_EXAMPLES) {
          await installExample(name, hubDir);
        }
        made.app = await startApp('eario-hub', [], { EARIO_HUB_DIR: hubDir });
        await page.goto(made.app.url);
        await expect(page.getByRole('heading', { level: 1, name: 'Eario hub' })).toBeVisible();
      });
    } finally {
      if (made.app !== undefined && made.hubDir !== undefined) {
        await stopHub(made.app, made.hubDir);
      } else if (made.hubDir !== undefined) {
        await fs.rm(made.hubDir, { recursive: true, force: true });
      }
    }
  },

  launch: async ({ page }, use) => {
    const started: ChildProcess[] = [];
    await use(async (name, args = []) => {
      const app = await startApp(name, args);
      started.push(app.process);
      await page.goto(app.url);
      await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
    });
    for (const child of started) {
      child.kill();
    }
  },
});

export { expect };
