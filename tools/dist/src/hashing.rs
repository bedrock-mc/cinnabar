use std::{fs, io::Read, path::Path};

use sha2::{Digest, Sha256};

use crate::{DistError, io_error};

pub(crate) fn hash_file(path: &Path) -> Result<(u64, String), DistError> {
    let mut file = fs::File::open(path).map_err(|source| io_error(path, source))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut bytes = 0_u64;
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|source| io_error(path, source))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
        bytes += read as u64;
    }
    Ok((bytes, format!("{:x}", digest.finalize())))
}
