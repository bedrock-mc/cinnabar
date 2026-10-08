use super::*;

/// Bounds local proof work before falling back to the complete search.
pub(super) const CAMERA_PROOF_BUDGET: usize = 64;

#[derive(Clone, Copy)]
struct ProbeNode {
    key: SubChunkKey,
    state: u8,
}

/// The new camera's exits must already be reached; its probe must recover all old camera exits.
/// These two inclusions prove that the complete reachable sets are equal.
pub(super) fn same_region(
    camera: SubChunkKey,
    grid: &ConnectivityGrid,
    scratch: &mut CaveVisibilityScratch,
) -> bool {
    let Some(previous) = scratch.camera else {
        return false;
    };
    let Some(bits) = grid.get(&camera).map(|value| value.bits()) else {
        return false;
    };
    let seeds = touched_faces(bits) as u8;
    let required = REACHED | seeds;
    if incremental::reached_state(camera, grid, scratch) & required != required {
        return false;
    }
    let Some(previous_bits) = grid.get(&previous).map(|value| value.bits()) else {
        return false;
    };
    let target = REACHED | touched_faces(previous_bits) as u8;
    // Each explored exit can discover at most one node and enqueue one new batch of exits.
    let mut nodes = [ProbeNode {
        key: camera,
        state: 0,
    }; CAMERA_PROOF_BUDGET + 1];
    let mut queue = [(0_usize, 0_u8); CAMERA_PROOF_BUDGET + 1];
    nodes[0].state = required;
    queue[0] = (0, seeds);
    let (mut count, mut head, mut tail) = (1, 0, 1);
    while head < tail {
        let (node, mut exits) = queue[head];
        head += 1;
        while exits != 0 {
            if scratch.work.proof_exits == CAMERA_PROOF_BUDGET {
                return false;
            }
            scratch.work.proof_exits += 1;
            let face = Face::ALL[exits.trailing_zeros() as usize];
            exits &= exits - 1;
            let Some(next) = adjacent(nodes[node].key, face) else {
                continue;
            };
            let Some(bits) = grid.get(&next).map(|value| value.bits()) else {
                continue;
            };
            let arrived = REACHED | ((bits >> (opposite(face) as u64 * 6)) & FACE_MASK) as u8;
            let index = nodes[..count]
                .iter()
                .position(|node| node.key == next)
                .unwrap_or(count);
            if index == count {
                nodes[index] = ProbeNode {
                    key: next,
                    state: 0,
                };
                count += 1;
            }
            let fresh = arrived & !nodes[index].state & FACE_MASK as u8;
            nodes[index].state |= arrived;
            if next == previous && nodes[index].state & target == target {
                return true;
            }
            if fresh != 0 {
                queue[tail] = (index, fresh);
                tail += 1;
            }
        }
    }
    false
}
