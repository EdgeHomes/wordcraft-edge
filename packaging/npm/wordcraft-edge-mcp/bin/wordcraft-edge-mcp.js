#!/usr/bin/env node
'use strict';
// Thin launcher only. npm needs a JS entry point to pick the right native binary;
// every tool, the MCP protocol and path confinement live in the Rust `wordcraft-cli mcp`.
const { spawn } = require('node:child_process');
const path = require('node:path');

const exe = process.platform === 'win32' ? 'wordcraft-cli.exe' : 'wordcraft-cli';
const pkg = `wordcraft-edge-mcp-${process.platform}-${process.arch}`;

function binary() {
  if (process.env.WORDCRAFT_CLI) return process.env.WORDCRAFT_CLI;
  try {
    return path.join(path.dirname(require.resolve(`${pkg}/package.json`)), 'bin', exe);
  } catch {
    process.stderr.write(
      `wordcraft-edge-mcp: no prebuilt binary for ${process.platform}-${process.arch} (${pkg} is not installed).\n` +
        'Build one with `cargo build -p wordcraft-cli --release` and set WORDCRAFT_CLI to its path.\n',
    );
    process.exit(1);
  }
}

// Confine the agent's file access: default to WORDCRAFT_ROOT, else the working directory,
// unless the caller passed a root of its own.
const args = process.argv.slice(2);
if (!args.some((a) => /^--root(=|$)/.test(a))) {
  args.push('--root', process.env.WORDCRAFT_ROOT || process.cwd());
}

const child = spawn(binary(), ['mcp', ...args], { stdio: 'inherit' });
for (const sig of ['SIGINT', 'SIGTERM', 'SIGHUP']) process.on(sig, () => child.kill(sig));
child.on('error', (e) => {
  process.stderr.write(`wordcraft-edge-mcp: could not start ${exe}: ${e.message}\n`);
  process.exit(1);
});
child.on('exit', (code, signal) => process.exit(code ?? (signal ? 1 : 0)));
