// Resource packs the player imports, kept in the browser's IndexedDB across visits. The client's
// Global Resources library reads and writes them by file name through `cinnabarPackStore`, and
// loads an archive only while it reads it.

const DATABASE = "cinnabar-resource-packs";
const FILES = "files";

let database = null;

/** The pack database, opened once. */
function open() {
  database ??= new Promise((resolve, reject) => {
    const request = indexedDB.open(DATABASE, 1);
    request.onupgradeneeded = () => request.result.createObjectStore(FILES);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
  return database;
}

/** Runs `work` on the file store in one transaction and resolves with its request's result. */
async function transact(mode, work) {
  const store = (await open()).transaction(FILES, mode).objectStore(FILES);
  return new Promise((resolve, reject) => {
    const request = work(store);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

/** Opens a file picker for pack archives; resolves with `[name, bytes]` per picked file. */
function pick(extensions) {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.multiple = true;
    input.accept = extensions.map((extension) => `.${extension}`).join(",");
    input.onchange = async () => resolve(await read(input.files));
    input.oncancel = () => resolve([]);
    input.click();
  });
}

/** `[name, bytes]` of each of `files`. */
async function read(files) {
  return Promise.all(
    Array.from(files, async (file) => [file.name, new Uint8Array(await file.arrayBuffer())]),
  );
}

/**
 * Installs the store the client uses. Archives are kept as Blobs, which browsers may hold on
 * disk, and read back only when the client asks.
 */
export function installPackStore(extensions) {
  globalThis.cinnabarPackStore = {
    names: () => transact("readonly", (store) => store.getAllKeys()),
    read: async (name) => {
      const blob = await transact("readonly", (store) => store.get(name));
      return blob ? new Uint8Array(await blob.arrayBuffer()) : null;
    },
    write: (name, bytes) => transact("readwrite", (store) => store.put(new Blob([bytes]), name)),
    remove: (name) => transact("readwrite", (store) => store.delete(name)),
    pick: () => pick(extensions),
  };
}

/** Imports pack files dropped anywhere on the page into the running client. */
export function acceptDroppedPacks(client) {
  const extensions = client.resource_pack_extensions();
  const isPack = (name) => extensions.some((extension) => name.toLowerCase().endsWith(`.${extension}`));
  document.addEventListener("dragover", (event) => event.preventDefault());
  document.addEventListener("drop", async (event) => {
    event.preventDefault();
    const files = Array.from(event.dataTransfer?.files ?? []).filter((file) => isPack(file.name));
    for (const [name, bytes] of await read(files)) {
      client.import_resource_pack(name, bytes);
    }
  });
}
