import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig, type Plugin } from 'vite';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, '../../..');
const LICENSES_FILE = 'licenses.json';

interface PackageLicense {
  name: string;
  version: string;
  license: string;
  text: string;
}

/** Splits a module path into the innermost package directory and name. */
function packageOf(id: string): { name: string; dir: string } | undefined {
  const marker = 'node_modules/';
  const at = id.lastIndexOf(marker);
  if (at < 0) return undefined;
  const rest = id.slice(at + marker.length).split('/');
  const name = rest[0]?.startsWith('@') ? `${rest[0]}/${rest[1]}` : rest[0];
  if (!name) return undefined;
  return { name, dir: id.slice(0, at + marker.length + name.length) };
}

function readLicense(dir: string): PackageLicense {
  const pkg = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8')) as { name: string; version: string; license?: string };
  const file = ['LICENSE', 'LICENSE.md', 'LICENSE.txt', 'license', 'LICENSE-MIT'].map((f) => join(dir, f)).find(existsSync);
  return { name: pkg.name, version: pkg.version, license: pkg.license ?? 'UNKNOWN', text: file ? readFileSync(file, 'utf8') : '' };
}

/** Emits notices for exactly the third-party packages whose code or files ship in the bundle. */
function bundledLicenses(): Plugin {
  return {
    name: 'cinnabar-bundled-licenses',
    configureServer(server) {
      server.middlewares.use(`/${LICENSES_FILE}`, (_req, res) => {
        res.setHeader('content-type', 'application/json');
        res.end('[]');
      });
    },
    generateBundle(_options, bundle) {
      const dirs = new Map<string, string>();
      for (const output of Object.values(bundle)) {
        const ids = output.type === 'chunk' ? Object.keys(output.modules) : (output.originalFileNames ?? []);
        for (const id of ids) {
          const pkg = packageOf(id.replace(/\\/g, '/'));
          if (pkg) dirs.set(pkg.name, pkg.dir);
        }
      }
      const notices = [...dirs.values()].map(readLicense).sort((a, b) => a.name.localeCompare(b.name));
      const unknown = notices.filter((n) => n.license === 'UNKNOWN').map((n) => n.name);
      if (unknown.length > 0) this.error(`bundled packages without a declared license: ${unknown.join(', ')}`);
      this.emitFile({ type: 'asset', fileName: LICENSES_FILE, source: JSON.stringify(notices) });
    },
  };
}

export default defineConfig({
  plugins: [svelte(), bundledLicenses()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    fs: { allow: [here, join(repoRoot, 'packaging/icons')] },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  build: {
    target: process.env.TAURI_ENV_PLATFORM === 'windows' ? 'chrome105' : 'safari15',
    sourcemap: Boolean(process.env.TAURI_ENV_DEBUG),
    // The 3D scene is one lazy chunk read from local disk after first paint.
    chunkSizeWarningLimit: 700,
  },
});
