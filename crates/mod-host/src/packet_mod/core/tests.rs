use super::*;

/// Encodes a canonical session pack list with archives placed after its descriptors.
fn pack_memory(archives: &[&[u8]]) -> Vec<u8> {
    let mut memory = vec![0; 8 + archives.len() * SESSION_PACK_BYTES as usize];
    memory[..4].copy_from_slice(&8_u32.to_le_bytes());
    memory[4..8].copy_from_slice(&(archives.len() as u32).to_le_bytes());
    for (index, archive) in archives.iter().enumerate() {
        let at = 8 + index * SESSION_PACK_BYTES as usize;
        let pointer = memory.len() as u32;
        memory[at..at + 4].copy_from_slice(&pointer.to_le_bytes());
        memory[at + 4..at + 8].copy_from_slice(&(archive.len() as u32).to_le_bytes());
        memory[at + 8] = u8::from(index == 0);
        memory.extend_from_slice(archive);
    }
    memory
}

#[test]
fn session_pack_descriptors_cannot_bypass_the_transfer_budget() {
    let memory = pack_memory(&[b"", b"", b""]);
    let mut reader = Reader {
        memory: &memory,
        budget: 3 * size_of::<SessionPack>() - 1,
    };
    assert!(reader.session_packs(0).is_err());
}

#[test]
fn session_pack_descriptors_and_archives_share_the_transfer_budget() {
    let memory = pack_memory(&[b"abc", b"de"]);
    let mut reader = Reader {
        memory: &memory,
        budget: 2 * size_of::<SessionPack>() + 4,
    };
    assert!(reader.session_packs(0).is_err());
}

#[test]
fn session_pack_descriptors_and_archives_can_fill_the_transfer_budget() {
    let memory = pack_memory(&[b"abc", b"de"]);
    let mut reader = Reader {
        memory: &memory,
        budget: 2 * size_of::<SessionPack>() + 5,
    };
    assert_eq!(
        reader.session_packs(0).unwrap(),
        [
            SessionPack {
                archive: b"abc".to_vec(),
                above_server: true,
            },
            SessionPack {
                archive: b"de".to_vec(),
                above_server: false,
            },
        ]
    );
    assert_eq!(reader.budget, 0);
}
