//! Item visuals carry only unconditional attachable placement.
use super::*;

#[test]
fn partial_artwork_item_transforms_do_not_bake_owner_mappings_in_either_order() {
    for mapped in ["fixture:a_tool", "zfixture:z_tool"] {
        let pack = equipment_pack();
        let root = pack.path();
        let original = fs::read(root.join("attachables/trident.entity.json")).unwrap();
        let mut definition: serde_json::Value = serde_json::from_slice(&original).unwrap();
        let description = &mut definition["minecraft:attachable"]["description"];
        description["identifier"] = mapped.into();
        description["item"] =
            serde_json::json!({"minecraft:trident":"q.owner_identifier == 'minecraft:player'"});
        description["animations"]["wield_third_person"] =
            "animation.mapped.wield_third_person".into();
        write(
            root,
            "attachables/mapped.json",
            &serde_json::to_vec(&definition).unwrap(),
        );
        write(root, "animations/mapped.json", br#"{"format_version":"1.10.0","animations":{"animation.mapped.wield_third_person":{"loop":true,"bones":{"pole":{"position":[40,0,0],"rotation":[0,0,0]}}}}}"#);
        let compilation = compile_entity_assets_with_report(root, MANIFEST).unwrap();
        let trident = visual(&compilation.assets.item_visuals, "minecraft:trident");
        assert_eq!(
            trident.third_person.translation.map(|s| s.get()),
            [1.5, -2.5, -10.5],
            "conditional {mapped} cannot determine context-free placement"
        );
    }
}
