# Retail protocol data

These compact tables are positive allowlists for Bedrock 1.26.50 (protocol 2193),
measured on public Bedrock Dedicated Server 1.26.52.3 (Linux archive SHA-256
`f6348d84fa714d04ca194f207e89453ca6bba0a1359396475271a52a150471c6`).

- `retail_items_1_26_50.tsv` contains the ItemRegistry network ID and identifier
  for each item referenced by the default CreativeContent packet. Its SHA-256 is
  `6f186e8f781c611722cd28ece47f643112732a89e18cd9beab9d414243750821`.
- `retail_biomes_1_26_50.txt` contains the biome identifiers of the default
  BiomeDefinitionList packet. Its SHA-256 is
  `6127c74c17455273bb5226f1e05e98709bc247c05a0137a8827cb97756c3b198`.
- `item_capacity_1_26_50.tsv` contains the measured `ItemStack.maxAmount` for
  every identifier in the retail item allowlist, using the documented
  [`ItemStack.maxAmount` API](https://learn.microsoft.com/en-us/minecraft/creator/scriptapi/minecraft/server/itemstack?view=minecraft-bedrock-stable#maxamount).
  Its SHA-256 is
  `58caa65a685f531787b9444e43d35c4d8bfafba551ae4ce9742fed24486aa6da`;
  `item_capacity_1_26_50.provenance.json` records the reproducibility inputs.

- `item_tags_native.tsv` contains native item-tag membership (one tag per line,
  space-separated members), generated from the shared protocolgen runtime
  catalog by `tools/registrygen/cmd/itemtags`. Its header records the source-lock
  SHA-256 and native source revision. Regenerate it with
  `go -C tools/registrygen run ./cmd/itemtags > crates/protocol/data/item_tags_native.tsv`.

Entries not established by those positive retail surfaces are omitted. Numeric
item IDs are preserved exactly; omissions therefore remain gaps.

The capacity table is a metadata-zero vanilla baseline, not a negotiated
runtime rule. Consumers must bind the active identifier and reconcile
server-provided item properties and component overrides before using it for
inventory behavior.
