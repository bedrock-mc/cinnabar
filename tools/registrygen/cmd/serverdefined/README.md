`serverdefined` generates the canonical ranges for vanilla blocks admitted to a
remote session only when the server supplies their definitions. It reads the
active target, its projection manifest and the pinned behavior pack downloaded
by `make assets`; source NBT and output registry must match their declared hashes.

From the repository root, verify the committed table with:

```sh
go -C tools/registrygen run ./cmd/serverdefined -check
```

Omit `-check` to regenerate it. `-states` selects a copy of the pinned source NBT
when it is unavailable in the Go module cache. The table preserves ranges for
states projected to reserved carrier records.
