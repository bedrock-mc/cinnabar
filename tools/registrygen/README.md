# Client registry generation

The active protocol-2193 generator reads the palette, per-state collision boxes,
light, friction and hardness from `protocolgen/generated/data`. Update its Go
module pin to update those facts. Generation rejects a different game target,
missing states or mismatched names. Registry and property catalogs must come
from the same source lock.

Rendering families, model selectors, retail admission, carrier encodings and
movement rules belong to Cinnabar. The reviewed protocol-1001 block carrier is
still the rendering-policy baseline. Its inherited collision and light facts
are replaced by exact state lookups in the shared catalog. Historical
protocol-1001 generation remains available for reproducing that baseline.

Run from the repository root:

```sh
go -C tools/registrygen run . \
  -block-v2193-legacy-breg ../../crates/assets/data/block-registry-v1001.bin \
  -block-v2193-allowlist ../../crates/protocol/data/retail_items_1_26_50.tsv \
  -out ../../crates/assets/data/block-registry-v2193.bin \
  -light-out ../../crates/assets/data/block-light-registry-v2193.bin \
  -block-v2193-manifest ../../assets/block-projection-v2193.json
python3 tools/registrygen/update_bindings.py --out .
go -C tools/registrygen run . \
  -physics-v2193-breg ../../crates/assets/data/block-registry-v2193.bin \
  -physics-v2193-out ../../crates/assets/data/block-physics-v2193.bin \
  -physics-v2193-manifest ../../assets/block-projection-v2193.json
go -C tools/registrygen run . \
  -block-item-breg ../../crates/assets/data/block-registry-v2193.bin \
  -block-item-out ../../crates/assets/data/block-item-routes-v2193.json
go -C tools/registrygen run . \
  -biome-v2193-allowlist ../../crates/protocol/data/retail_biomes_1_26_50.txt \
  -biome-out ../../crates/assets/data/biome-registry-v2193.bin \
  -biome-v2193-manifest ../../assets/biome-projection-v2193.json
python3 tools/registrygen/update_bindings.py --out .
go -C tools/registrygen test ./...
```

The destroy-table generator reads hardness and provisional tool classifications
from the same shared catalog. Missing tool evidence stays unresolved. Regenerate
it with `go -C tools/registrygen run ./cmd/blockdestroy -out ../../crates/sim/data/block_destroy_1_26_50.tsv`.

The active biome generator reads numeric IDs from the shared catalog and keeps
only names in the local retail allowlist. Its source lock is recorded in the
projection manifest. No local BDS executable or PMMP biome map is needed.
Client rendering policy and movement algorithms remain local.
