// Modules `build.mjs` provides at bundle time.

declare module 'virtual:wasm' {
  const bytes: Uint8Array;
  export default bytes;
}

// The compiled Cinnangles Sans carrier, when the build was given the font.
declare module 'virtual:font' {
  const bytes: Uint8Array | null;
  export default bytes;
}
