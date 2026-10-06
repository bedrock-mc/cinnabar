use super::{entity, suite::carrier_v4_fixture};

#[test]
fn geometry_specialized_animation_instances_round_trip() {
    let geometry_count = 64;
    let animation_count = 65;
    let clip_count = geometry_count * animation_count;
    let mut compiled = carrier_v4_fixture();
    let geometry = compiled.geometries[0].clone();
    let clip = compiled.animation_clips[0];
    let channel = compiled.animation_channels[0];
    let keyframe = compiled.animation_keyframes[0];
    let mut symbols = compiled
        .symbols
        .into_vec()
        .into_iter()
        .filter(|symbol| {
            !matches!(
                symbol.kind,
                entity::EntityAssetKind::Geometry | entity::EntityAssetKind::Animation
            )
        })
        .collect::<Vec<_>>();
    compiled.geometries = (0..geometry_count)
        .map(|index| {
            let identifier: Box<str> = format!("geometry.capacity.{index:03}").into();
            symbols.push(entity::EntityAssetSymbol {
                kind: entity::EntityAssetKind::Geometry,
                identifier: identifier.clone(),
                source_index: geometry.source_index,
                dependencies: Box::new([]),
            });
            entity::EntityGeometry {
                identifier,
                ..geometry.clone()
            }
        })
        .collect();
    let animations = (0..animation_count)
        .map(|index| {
            let identifier: Box<str> = format!("animation.capacity.{index:03}").into();
            symbols.push(entity::EntityAssetSymbol {
                kind: entity::EntityAssetKind::Animation,
                identifier: identifier.clone(),
                source_index: clip.source,
                dependencies: Box::new([]),
            });
            identifier
        })
        .collect::<Vec<_>>();
    symbols.sort_by(|left, right| {
        (left.kind, &left.identifier, left.source_index).cmp(&(
            right.kind,
            &right.identifier,
            right.source_index,
        ))
    });
    compiled.symbols = symbols.into_boxed_slice();
    let animation_symbols = animations
        .iter()
        .map(|identifier| {
            compiled
                .symbols
                .iter()
                .position(|symbol| symbol.identifier == *identifier)
                .unwrap() as u32
        })
        .collect::<Vec<_>>();
    compiled.animation_clips = (0..clip_count)
        .map(|index| entity::EntityAnimationClip {
            symbol: animation_symbols[index / geometry_count],
            geometry: Some((index % geometry_count) as u32),
            first_channel: index as u32,
            ..clip
        })
        .collect();
    compiled.animation_channels = (0..clip_count)
        .map(|index| entity::EntityAnimationChannel {
            first_keyframe: index as u32,
            ..channel
        })
        .collect();
    compiled.animation_keyframes = vec![keyframe; clip_count].into_boxed_slice();
    compiled.controllers = Box::new([]);
    compiled.controller_states = Box::new([]);
    compiled.controller_animations = Box::new([]);
    compiled.controller_transitions = Box::new([]);
    compiled.rig_bindings = Box::new([]);
    compiled.rig_geometries = Box::new([]);
    compiled.rig_animations = Box::new([]);
    compiled.rig_controllers = Box::new([]);
    compiled.item_visuals = Box::new([]);
    compiled.item_visual_aliases = Box::new([]);
    compiled.render = Default::default();

    let runtime = entity::RuntimeEntityAssets::from_compiled(compiled.clone())
        .expect("geometry-specific animation instances fit the retained carrier budget");
    assert_eq!(runtime.animation_clips().len(), clip_count);
    let blob = entity::encode_entity_blob(&compiled).unwrap();
    let decoded = entity::RuntimeEntityAssets::decode(&blob).unwrap();
    assert_eq!(decoded.animation_clips(), compiled.animation_clips.as_ref());
    assert_eq!(
        decoded.animation_channels(),
        compiled.animation_channels.as_ref()
    );
    assert_eq!(
        decoded.animation_keyframes(),
        compiled.animation_keyframes.as_ref()
    );
    assert_eq!(runtime.encode().unwrap(), blob);
    assert_eq!(decoded.encode().unwrap(), blob);
}
