// Bundles the plugin into dist/: code.js for Figma's main thread, and ui.html
// with the UI script, the editor's WebAssembly and the optional font inlined,
// since the plugin iframe cannot load files beside it.

import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = fileURLToPath(new URL('.', import.meta.url));
const pkg = `${here}pkg`;
if (!existsSync(`${pkg}/jsonui_editor_bg.wasm`)) {
  throw new Error('pkg/ is missing the editor bindings; run build.sh instead');
}

const assets = {
  name: 'assets',
  setup(build) {
    build.onResolve({ filter: /^virtual:wasm$/ }, () => ({ path: `${pkg}/jsonui_editor_bg.wasm` }));
    build.onResolve({ filter: /^virtual:font$/ }, () => ({ path: 'font', namespace: 'font' }));
    build.onLoad({ filter: /.*/, namespace: 'font' }, () => {
      const path = `${pkg}/cinnangles-sans.mcbefont`;
      return existsSync(path)
        ? { contents: readFileSync(path), loader: 'binary' }
        : { contents: 'export default null', loader: 'js' };
    });
  },
};

mkdirSync(`${here}dist`, { recursive: true });
await esbuild.build({
  entryPoints: [`${here}src/code.ts`],
  outfile: `${here}dist/code.js`,
  bundle: true,
  format: 'iife',
  target: 'es2017',
});
const ui = await esbuild.build({
  entryPoints: [`${here}src/ui.ts`],
  bundle: true,
  write: false,
  format: 'iife',
  target: 'es2020',
  loader: { '.wasm': 'binary' },
  plugins: [assets],
  // The bindings only read `import.meta.url` when not handed the module bytes.
  logOverride: { 'empty-import-meta': 'silent' },
});
const script = ui.outputFiles[0].text.replaceAll('</script', '<\\/script');
const page = readFileSync(`${here}src/ui.html`, 'utf8').replace('<!-- script -->', () => `<script>${script}</script>`);
writeFileSync(`${here}dist/ui.html`, page);
