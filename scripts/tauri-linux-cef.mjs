#!/usr/bin/env node
/**
 * Linux CEF build wrapper (aligned with Readest apps/readest-app/scripts/tauri.mjs).
 *
 * - Swaps src-tauri/Cargo.cef.lock <-> Cargo.lock for the duration of the command
 * - Invokes CEF-capable tauri CLI with --features cef and cargo
 *   --no-default-features --config src-tauri/.cargo/cef.toml
 *
 * Env:
 *   TAURI_CEF_CARGO=1  → use `cargo tauri` (CI installs from readest/tauri)
 *   TAURI_CEF_CLI=path → use a local unpacked cli-cef tauri.js
 *   otherwise          → npx -y @tauri-apps/cli-cef@3.0.0-alpha.27
 *
 * If killed mid-run: `mv src-tauri/Cargo.lock.wry src-tauri/Cargo.lock`
 */
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const CEF_CLI = '@tauri-apps/cli-cef@3.0.0-alpha.27';
const localCefCli = process.env['TAURI_CEF_CLI'];
const cargoCefCli = process.env['TAURI_CEF_CARGO'] === '1';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const srcTauri = path.join(root, 'src-tauri');
const cargoConfig = path.join(srcTauri, '.cargo/cef.toml');
const lockPath = path.join(srcTauri, 'Cargo.lock');
const cefLockPath = path.join(srcTauri, 'Cargo.cef.lock');
const savedLockPath = path.join(srcTauri, 'Cargo.lock.wry');

const [command, ...rest] = process.argv.slice(2);
if (!command) {
  console.error('usage: node scripts/tauri-linux-cef.mjs <dev|build|bundle> [tauri args] [-- cargo args]');
  process.exit(1);
}

const run = (file, args, cleanup = () => {}) => {
  const child = spawn(file, args, { stdio: 'inherit', cwd: root, env: process.env });
  child.on('exit', (code, signal) => {
    cleanup();
    process.exit(code ?? (signal ? 1 : 0));
  });
  child.on('error', (error) => {
    cleanup();
    console.error(error);
    process.exit(1);
  });
};

const split = rest.indexOf('--');
const tauriArgs = split === -1 ? rest : rest.slice(0, split);
const cargoArgs = split === -1 ? [] : rest.slice(split + 1);
const args = [
  command,
  '--features',
  'cef',
  ...tauriArgs,
  '--',
  '--no-default-features',
  '--config',
  cargoConfig,
  ...cargoArgs,
];

try {
  fs.writeFileSync(savedLockPath, fs.readFileSync(lockPath), { flag: 'wx' });
} catch (error) {
  if (error.code !== 'EEXIST') throw error;
  console.error(
    `${savedLockPath} exists: another CEF build is running, or an earlier one was killed. ` +
      'Wait, or run `mv src-tauri/Cargo.lock.wry src-tauri/Cargo.lock`.',
  );
  process.exit(1);
}
if (fs.existsSync(cefLockPath)) {
  fs.copyFileSync(cefLockPath, lockPath);
}
let swapped = true;
const restoreLock = () => {
  if (!swapped) return;
  fs.copyFileSync(lockPath, cefLockPath);
  fs.renameSync(savedLockPath, lockPath);
  swapped = false;
};

for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
  process.on(signal, () => {});
}

if (cargoCefCli) {
  run('cargo', ['tauri', ...args], restoreLock);
} else if (localCefCli) {
  run('node', [localCefCli, ...args], restoreLock);
} else {
  run('npx', ['-y', CEF_CLI, ...args], restoreLock);
}
