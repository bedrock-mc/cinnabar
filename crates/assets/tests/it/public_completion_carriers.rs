use assets::{
    CompiledFontCatalog, CompiledMolangExpression, EntityAnimationChannel, EntityAnimationClip,
    EntityAnimationController, EntityAnimationInterpolation, EntityAnimationKeyframe,
    EntityAnimationLoop, EntityAnimationProperty, EntityAssetSummary, EntityControllerAnimation,
    EntityControllerState, EntityControllerTransition, EntityRigAnimationBinding, EntityRigBinding,
    EntityRigControllerBinding, EntityRigFallback, EntityRigGeometryBinding, FONT_CARRIER_MAGIC,
    FONT_CARRIER_SCHEMA, FontCatalogError, FontCatalogIdentity, FontTexturePage, GlyphMetrics,
    ItemDisplayScalar, ItemDisplayTransform, ItemVisualAlias, ItemVisualDefinition,
    MAX_BLOCK_VISUALS, MAX_ENTITY_ANIMATION_CHANNELS, MAX_ENTITY_ANIMATION_CLIPS,
    MAX_ENTITY_ANIMATION_KEYFRAMES, MAX_ENTITY_CONTROLLER_ANIMATIONS, MAX_ENTITY_CONTROLLER_STATES,
    MAX_ENTITY_CONTROLLER_TRANSITIONS, MAX_ENTITY_CONTROLLERS, MAX_ENTITY_RIG_ANIMATIONS,
    MAX_ENTITY_RIG_BINDINGS, MAX_ENTITY_RIG_CONTROLLERS, MAX_ENTITY_RIG_GEOMETRIES,
    MAX_FONT_GLYPHS, MAX_FONT_PAGE_SIDE, MAX_FONT_PAGES, MAX_FONT_PATH_BYTES,
    MAX_FONT_SOURCE_BYTES, MAX_ITEM_IDENTIFIER_BYTES, MAX_ITEM_VISUAL_ALIASES, MAX_ITEM_VISUALS,
    MAX_MOLANG_COLLECTION_ITEMS, MAX_MOLANG_COLLECTION_ITEMS_TOTAL, MAX_MOLANG_COLLECTIONS,
    MAX_MOLANG_EXPRESSIONS, MAX_MOLANG_OPS, MAX_MOLANG_OPS_PER_EXPRESSION, MAX_MOLANG_STACK_DEPTH,
    MolangCollection, MolangCollectionItem, MolangOp, MolangSymbol, MolangSymbolKind,
    RuntimeFontCatalog, encode_font_catalog,
};

fn assert_public_type<T>() {}

#[test]
fn completion_carriers_are_available_only_through_the_assets_public_api() {
    assert_public_type::<CompiledMolangExpression>();
    assert_public_type::<EntityAnimationChannel>();
    assert_public_type::<EntityAnimationClip>();
    assert_public_type::<EntityAnimationController>();
    assert_public_type::<EntityAnimationInterpolation>();
    assert_public_type::<EntityAnimationKeyframe>();
    assert_public_type::<EntityAnimationLoop>();
    assert_public_type::<EntityAnimationProperty>();
    assert_public_type::<EntityAssetSummary>();
    assert_public_type::<EntityControllerAnimation>();
    assert_public_type::<EntityControllerState>();
    assert_public_type::<EntityControllerTransition>();
    assert_public_type::<EntityRigAnimationBinding>();
    assert_public_type::<EntityRigBinding>();
    assert_public_type::<EntityRigControllerBinding>();
    assert_public_type::<EntityRigFallback>();
    assert_public_type::<EntityRigGeometryBinding>();
    assert_public_type::<MolangCollection>();
    assert_public_type::<MolangCollectionItem>();
    assert_public_type::<MolangOp>();
    assert_public_type::<MolangSymbol>();
    assert_public_type::<MolangSymbolKind>();
    assert_public_type::<ItemDisplayScalar>();
    assert_public_type::<ItemDisplayTransform>();
    assert_public_type::<ItemVisualAlias>();
    assert_public_type::<ItemVisualDefinition>();
    assert_public_type::<CompiledFontCatalog>();
    assert_public_type::<RuntimeFontCatalog>();
    assert_public_type::<FontCatalogError>();
    assert_public_type::<FontCatalogIdentity>();
    assert_public_type::<FontTexturePage>();
    assert_public_type::<GlyphMetrics>();

    let _ = encode_font_catalog;
    assert_eq!(FONT_CARRIER_MAGIC, *b"MCBEFONT1");
    assert_eq!(FONT_CARRIER_SCHEMA, 1);
    let _ = (
        MAX_FONT_SOURCE_BYTES,
        MAX_FONT_PAGES,
        MAX_FONT_GLYPHS,
        MAX_FONT_PAGE_SIDE,
        MAX_FONT_PATH_BYTES,
        MAX_ENTITY_ANIMATION_CLIPS,
        MAX_ENTITY_ANIMATION_CHANNELS,
        MAX_ENTITY_ANIMATION_KEYFRAMES,
        MAX_ENTITY_CONTROLLERS,
        MAX_ENTITY_CONTROLLER_STATES,
        MAX_ENTITY_CONTROLLER_TRANSITIONS,
        MAX_ENTITY_CONTROLLER_ANIMATIONS,
        MAX_MOLANG_EXPRESSIONS,
        MAX_MOLANG_OPS_PER_EXPRESSION,
        MAX_MOLANG_OPS,
        MAX_MOLANG_STACK_DEPTH,
        MAX_MOLANG_COLLECTIONS,
        MAX_MOLANG_COLLECTION_ITEMS,
        MAX_MOLANG_COLLECTION_ITEMS_TOTAL,
        MAX_ENTITY_RIG_BINDINGS,
        MAX_ENTITY_RIG_GEOMETRIES,
        MAX_ENTITY_RIG_ANIMATIONS,
        MAX_ENTITY_RIG_CONTROLLERS,
        MAX_ITEM_VISUALS,
        MAX_ITEM_VISUAL_ALIASES,
        MAX_ITEM_IDENTIFIER_BYTES,
        MAX_BLOCK_VISUALS,
    );
}
