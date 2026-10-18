use std::{fs, io::Cursor, path::Path};

use ::image::{ImageFormat, ImageReader};
use assets::weather_textures::{
    END_SKY_SIDE, END_SKY_TEXTURE_PATH, WEATHER_SHEET_SIDE, WeatherImage, WeatherTextures,
    encode_weather_textures,
};

const MAX_SOURCE_BYTES: u64 = 64 * 1024;

fn read_png(root: &Path, relative: &str, side: u32) -> Result<WeatherImage, String> {
    let path = root.join(relative);
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
    if decoded.dimensions() != (side, side) {
        return Err(format!(
            "{} is {:?}, expected {side}x{side}",
            path.display(),
            decoded.dimensions()
        ));
    }
    Ok(WeatherImage {
        width: side,
        height: side,
        rgba8: decoded.into_raw().into_boxed_slice(),
    })
}

/// Reads the vanilla weather sheet and End sky from `pack` and encodes the MCBEWTH1 carrier.
pub fn compile_weather_textures(pack: &Path) -> Result<Vec<u8>, String> {
    let textures = WeatherTextures {
        weather: read_png(pack, "textures/environment/weather.png", WEATHER_SHEET_SIDE)?,
        end_sky: read_png(pack, &format!("{END_SKY_TEXTURE_PATH}.png"), END_SKY_SIDE)?,
    };
    encode_weather_textures(&textures).map_err(|error| error.to_string())
}

/// Compiles the carrier and writes it to `out` through a temporary sibling file.
pub fn compile_weather_textures_to_file(pack: &Path, out: &Path) -> Result<(), String> {
    let blob = compile_weather_textures(pack)?;
    assets::write_blob_atomic(out, &blob).map_err(|error| error.to_string())
}
