# Block-entity inventory generator

`blockentitygen` enumerates the concrete `world.NBTer` registrations from the
exact Dragonfly `b85c56ffea6b306798a935f14cc941c76618be52` module, rejects the
generic `world.unknownBlock` NBT passthrough, joins the result to the reviewed
renderer manifest, and writes the protocol-1001 inventory and coverage report.

The module is deliberately excluded from the repository `go.work`. The
workspace's `registrygen` module selects a newer Dragonfly revision, so this
generator must run with `GOWORK=off` to preserve its independent source pin:

```powershell
Push-Location tools/blockentitygen
$env:GOWORK = "off"
go test ./...
go run . `
  -renderer-manifest ../../assets/block-entity-renderers-v1001.json `
  -output ../../crates/assets/data/block-entities-v1001.json `
  -report ../../docs/block-entity-coverage-v1001-report.json
Pop-Location
```

Pass `-verify-bds <path>` to verify the local BDS 1.26.32.2 executable against
the pinned byte length and SHA-256 without copying it into the repository.
Pass `-strict-final` for the acceptance gate. It exits non-zero while any
renderer is deferred or unsupported, any required NBT variant lacks a witness,
or any drawable/no-draw route lacks evidence; failed strict-final runs publish
neither output.
