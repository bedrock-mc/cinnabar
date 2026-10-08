use super::super::PreviewTexture;
use super::*;

fn gear(scale: u16) -> PreviewEquipment {
    let (width, height) = (64 * scale, 32 * scale);
    let rgba = (0..height)
        .flat_map(|y| {
            (0..width).flat_map(move |x| [(x / scale) as u8 * 3, (y / scale) as u8 * 7, 90, 255])
        })
        .collect::<Vec<_>>();
    let texture = PreviewTexture {
        rgba: rgba.into(),
        width,
        height,
        tint: None,
    };
    PreviewEquipment {
        armor: std::array::from_fn(|_| Some(texture.clone())),
        ..Default::default()
    }
}

#[test]
fn hd_armor_keeps_the_same_cpu_preview_when_only_texel_density_changes() {
    let skin = vec![255; 64 * 64 * 4];
    let preview = |scale| {
        super::super::render(
            &skin,
            Default::default(),
            Default::default(),
            0.0,
            &gear(scale),
        )
    };
    assert_eq!(preview(1), preview(2));
}

#[test]
fn hd_armor_gpu_mesh_spans_the_scaled_atlas_region() {
    let model = |scale| {
        let armor = IconRef {
            page: 24,
            uv: [0, 0, 64 * scale, 32 * scale],
            glint: false,
        };
        mesh(
            Default::default(),
            Default::default(),
            0.0,
            IconRef {
                page: 23,
                uv: [0, 0, 64, 64],
                glint: false,
            },
            &gear(scale),
            [Some(armor); 4],
            [None; 2],
            false,
        )
        .unwrap()
    };
    let native = model(1);
    let hd = model(2);
    assert_eq!(native.vertices().len(), hd.vertices().len());
    let armor_start = native.batches()[1].index_range.start as usize;
    for (native, hd) in native.vertices()[armor_start..]
        .iter()
        .zip(&hd.vertices()[armor_start..])
    {
        assert_eq!(native.position, hd.position);
        assert_eq!(native.uv.map(|axis| axis * 2.0), hd.uv);
    }
}
