# Item binding regeneration

The checked-in table selects reviewed item coverage and its pinned source files.
The generator re-reads each definition's identifier and icon component, then
regenerates its alias, atlas variant and evidence hash. It checks the native
resource archive against the table's pinned hash before reading its entries.

Run from the repository root with the installed sample pack's full directory
and the matching native resource pack's `vanilla/__brarchive/items.brarchive`:

```sh
python3 tools/itembindinggen/main.py --table crates/assets/data/default-sprite-bindings-1.26.50.json --samples <full-samples> --archive <items.brarchive> --out <scratch-table.json>
cmp crates/assets/data/default-sprite-bindings-1.26.50.json <scratch-table.json>
```

After registrygen writes new carriers, refresh their derived JSON bindings with
`python3 tools/registrygen/update_bindings.py --out <repository-or-scratch-root>`.
Neither tool copies resource payloads into the repository.
