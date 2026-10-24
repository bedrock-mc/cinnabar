# Item capture fixture

On the local server, run `/fixture_items <filter> [index]`. The filter is a
case-insensitive part of an item identifier; `*` selects the entire registered
item list. Names sort alphabetically, then by metadata. Indices start at zero
and wrap at the end of the filtered list.

The command selects hotbar slot 1, puts the selected item in both hands, and
fills the nine hotbar slots with consecutive items from that index. It replaces
those slots and the offhand. Other inventory slots stay unchanged.

For example, `/fixture_items potion 0` starts with the first potion variant.
Capture a frame, then run `/fixture_items potion 1` for the next variant. Use
the same filter and index to reproduce the same equipment in another build.
The command reports the selected name, metadata, total count, and next index.
