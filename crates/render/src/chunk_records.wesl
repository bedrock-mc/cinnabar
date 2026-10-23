// Section placement and texture animation records shared by every terrain program.
@if(PACKED_TERRAIN)
import render::terrain_tables::{terrain_word, terrain_length};

struct ChunkOrigin { value: vec4<i32>, stream_bases: vec4<u32> }
// ANIMATION_GPU_LAYOUT
// Composable modules avoid member names ending in digits, which naga's writer would rename.
struct AnimationClockGpu { tick: u32, partial_tick: f32, reserved_a: u32, reserved_b: u32 }

@if(!PACKED_TERRAIN)
@group(0) @binding(2) var<storage, read> chunk_origins_data: array<ChunkOrigin>;

// Reads the same typed record from either terrain layout.
fn chunk_origins(index: u32) -> ChunkOrigin {
    @if(!PACKED_TERRAIN) return chunk_origins_data[index];
    @if(PACKED_TERRAIN) {
        if (index >= chunk_origins_length()) { var empty: ChunkOrigin; return empty; }
        let base = index * 8u;
        return ChunkOrigin(bitcast<vec4<i32>>(vec4(terrain_word(2u, base + 0u), terrain_word(2u, base + 1u), terrain_word(2u, base + 2u), terrain_word(2u, base + 3u))), vec4(terrain_word(2u, base + 4u), terrain_word(2u, base + 5u), terrain_word(2u, base + 6u), terrain_word(2u, base + 7u)));
    }
}

// Returns the record count without including adjacent packed tables.
fn chunk_origins_length() -> u32 {
    @if(!PACKED_TERRAIN) return arrayLength(&chunk_origins_data);
    @if(PACKED_TERRAIN) return terrain_length(2u) / 8u;
}
@if(!PACKED_TERRAIN)
@group(0) @binding(9) var<storage, read> animations_data: array<AnimationGpu>;

// Reads the same typed record from either terrain layout.
fn animations(index: u32) -> AnimationGpu {
    @if(!PACKED_TERRAIN) return animations_data[index];
    @if(PACKED_TERRAIN) {
        if (index >= animations_length()) { var empty: AnimationGpu; return empty; }
        let base = index * 4u;
        return AnimationGpu(terrain_word(9u, base + 0u), terrain_word(9u, base + 1u), terrain_word(9u, base + 2u), terrain_word(9u, base + 3u));
    }
}

// Returns the record count without including adjacent packed tables.
fn animations_length() -> u32 {
    @if(!PACKED_TERRAIN) return arrayLength(&animations_data);
    @if(PACKED_TERRAIN) return terrain_length(9u) / 4u;
}
@if(!PACKED_TERRAIN)
@group(0) @binding(10) var<storage, read> animation_frames_data: array<u32>;

// Reads the same typed record from either terrain layout.
fn animation_frames(index: u32) -> u32 {
    @if(!PACKED_TERRAIN) return animation_frames_data[index];
    @if(PACKED_TERRAIN) {
        if (index >= animation_frames_length()) { var empty: u32; return empty; }
        let base = index * 1u;
        return terrain_word(10u, base + 0u);
    }
}

// Returns the record count without including adjacent packed tables.
fn animation_frames_length() -> u32 {
    @if(!PACKED_TERRAIN) return arrayLength(&animation_frames_data);
    @if(PACKED_TERRAIN) return terrain_length(10u) / 1u;
}
@group(0) @binding(11) var<uniform> clock: AnimationClockGpu;
