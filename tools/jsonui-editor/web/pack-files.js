// Browser File handles stay lazy until the workspace requests their bytes.
const EAGER = /(^|\/)(manifest\.json|ui\/_ui_defs\.json|ui\/_global_variables\.json|texts\/en_US\.lang|textures\/.*\.json)$/i;

/** Stage a folder and key its pending File handles by the workspace's normalized paths. */
export async function loadFolder(editor, name, entries) {
  const index = editor.add_layer(name);
  const hostFiles = new Map();
  for (const [path, file] of entries) {
    if (EAGER.test(path)) {
      editor.stage_file(path, new Uint8Array(await file.arrayBuffer()));
    } else {
      editor.stage_pending(path);
      hostFiles.set(path, file);
    }
  }
  const paths = JSON.parse(editor.commit_files(index));
  return { index, files: new Map(paths.map(([path, host]) => [path, hostFiles.get(host)])) };
}

/** Supply exact pending paths once, leaving loaded or edited bytes in the workspace. */
export async function supplyFiles(editor, layers, wanted) {
  let supplied = 0;
  for (const [layer, path] of wanted) {
    const files = layers[layer]?.files;
    const file = files?.get(path);
    if (!file) continue;
    const bytes = new Uint8Array(await file.arrayBuffer());
    // Ignore reads completed after the pending handle or its layer was replaced.
    if (layers[layer]?.files !== files || files.get(path) !== file) continue;
    editor.supply(layer, path, bytes);
    files.delete(path);
    supplied++;
  }
  return supplied;
}

/** Load remaining folder bytes only for a full export of a confirmed owned layer. */
export async function supplyExportFiles(editor, layers, request) {
  if (request.mode !== "full" || !request.own_layer) return;
  const selected = layers[request.layer];
  const paths = [...(selected?.files.keys() ?? [])];
  await supplyFiles(editor, layers, paths.map((path) => [request.layer, path]));
  if (layers[request.layer] !== selected) throw new Error("The selected layer changed. Choose it again before exporting.");
}
