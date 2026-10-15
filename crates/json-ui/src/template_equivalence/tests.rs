use super::*;

const SCREEN: &str = crate::vanilla::Reference::CraftingInventoryScreen.as_str();

fn catalog() -> Catalog {
    Catalog::from_files([
        ("ui/_global_variables.json", br#"{"$cell_width":18}"#.as_slice()),
        ("ui/_ui_defs.json", br#"{"ui_defs":["ui/fixture.json","ui/crafting_screen.json"]}"#.as_slice()),
        ("ui/crafting_screen.json", br#"{"namespace":"crafting","inventory_screen@fixture.screen":{}}"#.as_slice()),
        ("ui/fixture.json", br#"{
            "namespace":"fixture",
            "screen":{"type":"screen","controls":[{"grid":{
                "type":"grid","grid_dimensions":[9,100],
                "collection_name":"items","grid_item_template":"fixture.cell"
            }}]},
            "cell":{"type":"panel","size":["$cell_width",18],"controls":[{"label@fixture.label":{}}]},
            "label":{"type":"label","text":"Item"}
        }"#.as_slice()),
    ]).unwrap()
}

fn unchanged(current: &Catalog, baseline: &Catalog) -> bool {
    let context = Context::default();
    let resolved = crate::resolve_screen(SCREEN, current, &context).unwrap();
    screen_templates_unchanged(
        &resolved,
        SCREEN,
        current,
        baseline,
        &context,
        &crate::DataSource::new(),
    )
}

#[test]
fn unrelated_controls_preserve_grid_template_equivalence() {
    let baseline = catalog();
    let mut current = baseline.clone();
    current.overlay_text(
        "ui/hud.json",
        r#"{"namespace":"hud","label":{"type":"label","text":"Server"}}"#,
    );
    assert!(unchanged(&current, &baseline));
}

#[test]
fn inherited_and_deferred_template_changes_are_detected() {
    let baseline = catalog();
    for patch in [
        r#"{"namespace":"fixture","label":{"text":"Changed"}}"#,
        r#"{"namespace":"fixture","cell":{"offset":[0,-100]}}"#,
        r#"{"namespace":"fixture","screen":{"size":[500,500]}}"#,
    ] {
        let mut current = baseline.clone();
        current.overlay_text("ui/fixture.json", patch);
        assert!(!unchanged(&current, &baseline), "{patch}");
    }
    let mut current = baseline.clone();
    current.overlay_globals_text(r#"{"$cell_width":36}"#);
    assert!(!unchanged(&current, &baseline));
}

#[test]
fn data_dependent_factories_and_recursive_grids_remain_conservative() {
    for patch in [
        r#"{"namespace":"fixture","cell":{"type":"slider","factory":{"name":"dynamic","control_ids":{"row":"fixture.label"}}}}"#,
        r#"{"namespace":"fixture","cell":{"type":"grid","grid_dimensions":[1,1],"grid_item_template":"fixture.cell"}}"#,
    ] {
        let mut baseline = catalog();
        baseline.overlay_text("ui/fixture.json", patch);
        assert!(!unchanged(&baseline, &baseline));
    }
}

#[test]
fn single_template_factories_compare_their_scoped_children() {
    let mut baseline = catalog();
    baseline.overlay_text("ui/fixture.json", r#"{"namespace":"fixture","cell":{"controls":[],"collection_name":"details","factory":{"name":"details","control_name":"fixture.label"}}}"#);
    assert!(unchanged(&baseline, &baseline));
    let mut current = baseline.clone();
    current.overlay_text(
        "ui/fixture.json",
        r#"{"namespace":"fixture","label":{"offset":[0,100]}}"#,
    );
    assert!(!unchanged(&current, &baseline));
}

#[test]
fn supplied_factory_variables_cannot_reuse_a_static_comparison() {
    let catalog = catalog();
    let context = Context::default();
    let resolved = crate::resolve_screen(SCREEN, &catalog, &context).unwrap();
    for screen_role in [false, true] {
        let mut data = crate::DataSource::new();
        if screen_role {
            data.set_factory_id("row");
        } else {
            data.set_factory("dynamic", Vec::new());
        }
        assert!(!screen_templates_unchanged(
            &resolved, SCREEN, &catalog, &catalog, &context, &data
        ));
    }
}
