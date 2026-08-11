#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    // Admission is bounded and read-only. Both success and every typed rejection
    // are valid outcomes; the parser must never eagerly extract arbitrary files.
    let _ = resource_pack::validate_archive_bytes(bytes);
});
