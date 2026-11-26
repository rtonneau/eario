import { type ChildProcess, spawn } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { test as base, expect } from '@playwright/test';

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');

/** Directory of the example executables; `cargo xtask e2e` builds them there. */
const binDir = process.env.EARIO_E2E_BIN_DIR ?? path.join(repositoryRoot, 'target', 'debug');

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

/**
 * Starts the example `name` with `args`, on a port chosen by the system and
 * without opening a browser, and resolves with the address it prints.
 */
function startApp(name: string, args: string[]): Promise<RunningApp> {
  const executable = path.join(binDir, process.platform === 'win32' ? `${name}.exe` : name);
  const child = spawn(executable, args, {
    cwd: repositoryRoot,
    env: { ...process.env, EARIO_NO_BROWSER: '1' },
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

/** Tests get `launch`, which starts an example and opens it in the page. */
export const test = base.extend<{ launch: (name: string, args?: string[]) => Promise<void> }>({
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
