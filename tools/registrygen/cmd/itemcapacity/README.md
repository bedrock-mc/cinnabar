# Item capacity generator

This command projects a pinned item-capacity measurement onto the repository's retail item
allowlist. It accepts only the complete Bedrock Dedicated Server 1.26.40.8 measurement used
for protocol 2168 and writes deterministic TSV and provenance files.

To repeat the measurement, copy `probe/manifest.json` and `probe/main.js` into a behavior
pack (`manifest.json` and `scripts/main.js` respectively), enable that pack in a local world,
start the matching public dedicated server, and retain its complete log. The probe uses the
documented `ItemStack.maxAmount` property and does not modify inventories. See the
[ItemStack API](https://learn.microsoft.com/en-us/minecraft/creator/scriptapi/minecraft/server/itemstack?view=minecraft-bedrock-stable#maxamount).

Run the generator with all paths explicit:

```text
go run ./cmd/itemcapacity -probe-log <server-log> -retail-items <retail-items.tsv> -out <capacity.tsv> -provenance-out <provenance.json>
```

The command records the independently measured public-binary hash in the provenance and
verifies the exact vetted log hash, probe headers, completion counts, row validity, and
complete allowlist coverage. It does not read or hash a server executable. It never fetches
or installs a server and never publishes measured identifiers outside the retail allowlist.

A freshly repeated measurement has different timestamps and session details, so its complete
log will not match the pinned log hash. Accepting a new measurement requires reviewing it and
explicitly updating the pinned hash and provenance; the unchanged command intentionally only
replays the vetted measurement.
