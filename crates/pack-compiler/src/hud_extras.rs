use std::{fs, io::Cursor, path::Path};

use ::image::{ImageFormat, ImageReader};
use assets::{HUD_EXTRA_SIDE, HudExtraRole, HudExtras, encode_hud_extras};

const MAX_SOURCE_BYTES: u64 = 16 * 1024;

fn read_sprite(pack: &Path, role: HudExtraRole) -> Result<Box<[u8]>, String> {
    let path = pack.join(role.source_path());
    let metadata = fs::metadata(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    if metadata.len() > MAX_SOURCE_BYTES {
        return Err(format!(
            "{} exceeds {MAX_SOURCE_BYTES} bytes",
            path.display()
        ));
    }
    let bytes = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let decoded = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png)
        .decode()
        .map_err(|error| format!("{}: {error}", path.display()))?
        .into_rgba8();
    if decoded.dimensions() != (HUD_EXTRA_SIDE, HUD_EXTRA_SIDE) {
        return Err(format!(
            "{} is {:?}, expected {HUD_EXTRA_SIDE}x{HUD_EXTRA_SIDE}",
            path.display(),
            decoded.dimensions()
        ));
    }
    Ok(decoded.into_raw().into_boxed_slice())
}

/// Reads the hardcore heart sprites from `pack` and writes the MCBEHXT1 carrier to `out`.
pub fn compile_hud_extras_to_file(pack: &Path, out: &Path) -> Result<(), String> {
    let images = HudExtraRole::ALL
        .iter()
        .map(|role| read_sprite(pack, *role))
        .collect::<Result<Vec<_>, _>>()?;
    let extras = HudExtras::new(images).map_err(|error| error.to_string())?;
    let blob = encode_hud_extras(&extras);
    let temporary = out.with_extension("tmp");
    fs::write(&temporary, &blob).map_err(|error| format!("{}: {error}", temporary.display()))?;
    fs::rename(&temporary, out).map_err(|error| format!("{}: {error}", out.display()))
}
