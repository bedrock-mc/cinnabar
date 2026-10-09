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
python3 tools/registrygen/update_bindings.py --out .
go -C tools/registrygen test ./...
```

The destroy-table generator also reads shared hardness. It still needs the
pinned Prismarine tool and harvest classifications in `assets/block-data-sources.json`.
Those classifications are provisional; missing evidence stays unresolved.
Its `-manifest` and `-bundle` flags now select only the tool evidence.

The shared biome catalog has climate and presentation fields but no numeric
biome IDs. The current biome ID capture and its cross-checks remain necessary.
Shared block data also does not supply preferred mining tools, harvest tiers,
client rendering policy or movement algorithms.
