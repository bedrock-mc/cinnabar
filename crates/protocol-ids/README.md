# Protocol IDs

This dependency-free `no_std` crate owns named wire IDs and the pinned fallback
air identity. Consumers import it directly, including WebAssembly tools.

`data/protocol-ids.json` contains the numeric groups and fixed window capacities
checked by `tools/fixturegen`. Its `-ids-out` option regenerates that fixture.
`data/actor-data-extensions.json` adds the third flag-word contract.
`data/movement-contract.json` owns movement attribute names, the sprint modifier
identity, and word-specific flags and effects missing from those numeric inputs.

Run `python3 tools/fixturegen/generate_ids.py` after updating these inputs.
It reads enchantments from the vendored codec and air from the hash-checked
active block registry, then writes `src/generated.rs`. The fixture generator
tests reject drift in both the numeric fixture and generated declarations.
