# Retail protocol data

These compact tables are positive allowlists for Bedrock 1.26.40.

- `retail_items_1_26_40.tsv` contains the ItemRegistry network ID and identifier
  for each item referenced by the default CreativeContent packet from public
  Bedrock Dedicated Server 1.26.40.8. Its SHA-256 is
  `ee8917e7293c89469d6d114cad634eac0b45a702a1d73e2edddd6d5eeee725d0`.
- `retail_biomes_1_26_40.txt` contains the confirmed retail biome identifier
  set. Its SHA-256 is
  `df7e18c18e939e21f387838479ee9c79b0d7eb798fb1bd906f51c56963058574`.
- `item_capacity_1_26_40.tsv` contains the measured `ItemStack.maxAmount` for
  every identifier in the retail item allowlist. The measurement used public
  Bedrock Dedicated Server 1.26.40.8 and the documented
  [`ItemStack.maxAmount` API](https://learn.microsoft.com/en-us/minecraft/creator/scriptapi/minecraft/server/itemstack?view=minecraft-bedrock-stable#maxamount).
  Its SHA-256 is
  `a494566eaf96fb57a38a736a1ec02d54424669e9272ae4c889be39c6f3e9caf3`;
  `item_capacity_1_26_40.provenance.json` records the reproducibility inputs.

Entries not established by those positive retail surfaces are omitted. Numeric
item IDs are preserved exactly; omissions therefore remain gaps.

The capacity table is a metadata-zero vanilla baseline, not a negotiated
runtime rule. Consumers must bind the active identifier and reconcile
server-provided item properties and component overrides before using it for
inventory behavior.
