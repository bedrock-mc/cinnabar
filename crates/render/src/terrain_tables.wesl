// The header stores a word offset and length at each original binding number.
@group(0) @binding(2) var<storage, read> terrain_tables: array<u32>;

// Keeps bounds checks inside each original table after packing.
fn terrain_length(binding: u32) -> u32 {
    return terrain_tables[binding * 2u + 1u];
}

// Reads one word without allowing an invalid index to enter the next table.
fn terrain_word(binding: u32, index: u32) -> u32 {
    if (index >= terrain_length(binding)) { return 0u; }
    return terrain_tables[terrain_tables[binding * 2u] + index];
}

// Checks the record before multiplying, so a bad index cannot wrap into a valid record.
fn terrain_field(binding: u32, index: u32, stride: u32, field: u32) -> u32 {
    if (index >= terrain_length(binding) / stride || field >= stride) { return 0u; }
    return terrain_word(binding, index * stride + field);
}
