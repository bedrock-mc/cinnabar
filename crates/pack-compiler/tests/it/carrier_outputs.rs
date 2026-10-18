use std::{fs, path::Path};

use assets::{
    carriers::{HUD_EXTRAS, WEATHER},
    hud_extras::{HUD_EXTRA_SIDE, HudExtraRole, decode_hud_extras},
    weather_textures::{
        END_SKY_SIDE, END_SKY_TEXTURE_PATH, WEATHER_SHEET_SIDE, decode_weather_textures,
    },
};

/// Writes an original solid-color sprite at the carrier's declared source path.
fn sprite(pack: &Path, name: &str, side: u32, color: [u8; 4]) {
    let path = pack.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::RgbaImage::from_pixel(side, side, image::Rgba(color))
        .save(path)
        .unwrap();
}

#[test]
fn parallel_carriers_preserve_other_temporary_files_and_publish_their_own_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("pack");
    let out = dir.path().join("out");
    fs::create_dir_all(&out).unwrap();
    let weather_color = [11, 22, 33, 255];
    let heart_color = [44, 55, 66, 255];
    sprite(
        &pack,
        "textures/environment/weather.png",
        WEATHER_SHEET_SIDE,
        weather_color,
    );
    sprite(
        &pack,
        &format!("{END_SKY_TEXTURE_PATH}.png"),
        END_SKY_SIDE,
        weather_color,
    );
    for role in HudExtraRole::ALL {
        sprite(&pack, role.source_path(), HUD_EXTRA_SIDE, heart_color);
    }
    let weather = out.join(WEATHER.output);
    let hearts = out.join(HUD_EXTRAS.output);
    // Another writer may already own this sibling, including during a parallel prepare run.
    let occupied = weather.with_extension("tmp");
    fs::write(&occupied, b"another writer").unwrap();
    std::thread::scope(|scope| {
        let weather_job =
            scope.spawn(|| pack_compiler::compile_weather_textures_to_file(&pack, &weather));
        let heart_job = scope.spawn(|| pack_compiler::compile_hud_extras_to_file(&pack, &hearts));
        weather_job.join().unwrap().unwrap();
        heart_job.join().unwrap().unwrap();
    });
    let weather = decode_weather_textures(&fs::read(weather).unwrap())
        .unwrap()
        .0;
    assert_eq!(&weather.weather.rgba8[..4], &weather_color);
    let hearts = decode_hud_extras(&fs::read(hearts).unwrap()).unwrap().0;
    assert_eq!(&hearts.rgba8(HudExtraRole::HeartFull)[..4], &heart_color);
    assert_eq!(fs::read(occupied).unwrap(), b"another writer");
}
