# Client registry generation

The active protocol-2193 generator reads the palette, per-state collision boxes,
light, friction and hardness from `protocolgen/generated/data`. Update its Go
module pin to update those facts. Generation rejects a different game target,
missing states or mismatched names. Registry and property catalogs must come
from the same source lock.

Rendering families, model selectors, retail admission, carrier encodings and
movement rules belong to Cinnabar. `crates/assets/data/block-family-routing.json`
owns crossed-plane routes that older carriers did not record. The generator
writes those families, and historical carrier admission uses the same catalog.
The pack compiler consumes the recorded family without repairing it by name. The reviewed protocol-1001 block carrier is
still the rendering-policy baseline. Its inherited collision and light facts
are replaced by exact state lookups in the shared catalog. Historical
protocol-1001 generation remains available for reproducing that baseline.

Run from the repository root:

```sh
mkdir -p .local/registry
# Go exports library facts; registryc applies the client policy and constructs carriers.
go -C tools/registrygen run . -facts-out ../../.local/registry/facts.json
cargo run --locked -p asset-compiler --bin registryc -- \
  --root . --facts .local/registry/facts.json --kind all \
  --out .local/registry/regenerated --check
```

`--check` regenerates block, light, biome and movement-seed carriers, their checksum
sidecars and projection manifests in the selected directory, then compares their
exact bytes with the committed outputs. CI runs the same check through the
asset compiler's integration suite. The existing Go generation flags delegate
the active passes to Rust, preserving callers that use explicit output paths.
Historical encoders and the independent projection fixtures remain available.

The block carrier also records `BlockTraits` and `SurvivalRule`. Ordered selectors
in `registry-policy.json` own model families and native membership rules. The
compiler rejects unknown members and selectors that match no pinned states.
`facts-expansion-v2193.tsv` shows trait membership and provisional rules;
`vocabulary-v2193.txt` combines the pinned identifier, property and tag domains.
Runtime consumers read dense block facts and typed properties without matching names.

Property domains combine the hash-checked sample metadata with valid values in
the pinned palette, including Education states omitted from the samples. Blocks
whose states use fewer values than a shared property's domain get their own domain.
Selected behavior-pack components come from the same archive. Regenerate these inputs
before the schema and carriers; pass `--facts` to reuse an existing pinned export:

```sh
python3 tools/registrygen/generate_block_inputs.py --archive <pinned-sample-archive>
python3 tools/registrygen/generate_schema.py
```

The Rust registry pass writes the movement seed. The asset compiler adds independent
selection geometry and final state facts. Update each physics file’s `.sha256`
sidecar after changing it. `make assets` builds the final carrier into the local
compiled asset directory.

The destroy-table generator reads hardness and provisional tool classifications
from the same shared catalog. Missing tool evidence stays unresolved. Regenerate
it with `go -C tools/registrygen run ./cmd/blockdestroy -out ../../crates/assets/data/block-destroy-v2193.tsv`.

The active biome generator reads numeric IDs from the shared catalog and keeps
only names in the local retail allowlist. Its source lock is recorded in the
projection manifest. No local BDS executable or PMMP biome map is needed.
Client rendering policy and movement algorithms remain local.

The registry wire contract is `crates/assets/data/registry-schema.json`. It owns
the byte order, discriminants, source flags, compiled enrichment flags and limits.
After editing it, run `python3 tools/registrygen/generate_schema.py` from the root
with Python 3, Go and Rust installed. The Go tests check that both generated
declarations are current and share the synthetic fixture used by the Rust reader.
Changing the wire layout also requires a new carrier version and regenerated carriers.
