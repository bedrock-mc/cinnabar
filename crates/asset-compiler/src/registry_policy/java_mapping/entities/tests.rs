use super::*;

const DEFINITIONS: &str = r#"
public class BedrockEntityDefinitions {
    public static final BedrockEntityDefinition BOAT;
    public static final BedrockEntityDefinition EVOCATION_ILLAGER;
    public static final BedrockEntityDefinition SPLASH;

    static {
        BOAT = BedrockEntityDefinition.ofVanilla(Identifier.of("boat"));
        EVOCATION_ILLAGER = BedrockEntityDefinition.ofVanilla(Identifier.of("evocation_illager"));
        SPLASH = BedrockEntityDefinition.ofVanilla(Identifier.of("custom", "splash"));
    }
}
"#;

/// `VanillaEntities.java` syntax: plain builders, a `bedrockIdentifier` before and after
/// `.type`, definition indirection, inheritance from another type, a helper method, an
/// unregistered builder and an entity base that is not a type.
const VANILLA: &str = r#"
public final class VanillaEntities {
    public static final VanillaEntityType<CowEntity> COW;
    public static final VanillaEntityType<EnderCrystalEntity> END_CRYSTAL;
    public static final VanillaEntityType<SpellcasterIllagerEntity> EVOKER;
    public static final VanillaEntityType<SpellcasterIllagerEntity> ILLUSIONER;
    public static final VanillaEntityType<ZombieEntity> ZOMBIE;
    public static final VanillaEntityType<ZombieEntity> HUSK;
    public static final VanillaEntityType<BoatEntity> OAK_BOAT;
    public static final VanillaEntityType<PotionEntity> SPLASH_POTION;
    public static final VanillaEntityType<EnderDragonPartEntity> ENDER_DRAGON_PART;

    static {
        EntityTypeBase<Entity> entityBase = EntityTypeDefinition.baseBuilder(Entity.class)
                .addTranslator(MetadataTypes.POSE, (entity, entityMetadata) -> entity.setPose(entityMetadata.getValue()))
                .build();
        COW = VanillaEntityType.inherited(CowEntity::new, entityBase)
                .type(EntityType.COW) // the Java id is the Bedrock id
                .height(1.4f).width(0.9f)
                .build();
        END_CRYSTAL = VanillaEntityType.inherited(EnderCrystalEntity::new, entityBase)
                .bedrockIdentifier("minecraft:ignored")
                .type(EntityType.END_CRYSTAL)
                .bedrockIdentifier("minecraft:ender_crystal")
                .build();
        EVOKER = VanillaEntityType.inherited(SpellcasterIllagerEntity::new, entityBase)
                .type(EntityType.EVOKER)
                .bedrockDefinition(BedrockEntityDefinitions.EVOCATION_ILLAGER)
                .build();
        ILLUSIONER = VanillaEntityType.inherited(SpellcasterIllagerEntity::new, entityBase)
                .type(EntityType.ILLUSIONER)
                .bedrockIdentifier("minecraft:overridden")
                .bedrockDefinition(BedrockEntityDefinitions.EVOCATION_ILLAGER)
                .build();
        ZOMBIE = VanillaEntityType.inherited(ZombieEntity::new, entityBase)
                .type(EntityType.ZOMBIE)
                .bedrockIdentifier("zombie_v2")
                .build();
        HUSK = VanillaEntityType.inherited(ZOMBIE.factory(), ZOMBIE)
                .type(EntityType.HUSK)
                .build();
        OAK_BOAT = buildBoat(entityBase, EntityType.OAK_BOAT, BoatEntity.BoatVariant.OAK);
        EntityTypeBase<AbstractCubeEntity> cubeBase = VanillaEntityType.baseInherited(AbstractCubeEntity.class, entityBase)
                .build();
        VanillaEntityType<PotionEntity> potionBase = VanillaEntityType.inherited(PotionEntity::new, entityBase)
                .build(false);
        SPLASH_POTION = VanillaEntityType.<PotionEntity>inherited(null, potionBase)
                .type(EntityType.SPLASH_POTION)
                .bedrockDefinition(BedrockEntityDefinitions.SPLASH)
                .build();
        ENDER_DRAGON_PART = VanillaEntityType.<EnderDragonPartEntity>builder(null)
                .bedrockDefinition(BedrockEntityDefinitions.BOAT)
                .build(false); /* never sent */
        Registries.JAVA_ENTITY_IDENTIFIERS.get().put("minecraft:marker", null);
    }

    private static VanillaEntityType<BoatEntity> buildBoat(EntityTypeBase<BoatEntity> base, EntityType EntityType, BoatEntity.BoatVariant variant) {
        return VanillaEntityType.inherited(context -> new BoatEntity(context, variant), base)
            .type(EntityType)
            .bedrockDefinition(BedrockEntityDefinitions.BOAT)
            .build();
    }
}
"#;

fn pairs(vanilla: &str) -> Result<Vec<(String, String)>> {
    entity_pairs(vanilla, DEFINITIONS)
}

#[test]
fn builder_chains_resolve_like_geyser_builders() {
    let pairs = pairs(VANILLA).unwrap();
    let expected = [
        ("cow", "minecraft:cow"),
        ("end_crystal", "minecraft:ender_crystal"),
        ("evoker", "minecraft:evocation_illager"),
        ("illusioner", "minecraft:evocation_illager"),
        ("zombie", "minecraft:zombie_v2"),
        ("husk", "minecraft:husk"),
        ("oak_boat", "minecraft:boat"),
        ("splash_potion", "custom:splash"),
    ]
    .map(|(java, bedrock)| (format!("minecraft:{java}"), bedrock.to_owned()));
    assert_eq!(pairs, expected);
}

#[test]
fn an_unparseable_builder_fails_the_generator() {
    for (broken, what) in [
        (
            ".type(EntityType.COW) // the Java id",
            ".type(EntityType.COW, extra) // the Java id",
        ),
        (
            ".bedrockIdentifier(\"minecraft:ender_crystal\")",
            ".bedrockIdentifier(name)",
        ),
        (
            ".bedrockDefinition(BedrockEntityDefinitions.EVOCATION_ILLAGER)\n                .build();\n        ILLUSIONER",
            ".bedrockDefinition(BedrockEntityDefinitions.MISSING)\n                .build();\n        ILLUSIONER",
        ),
        (
            "OAK_BOAT = buildBoat(entityBase, EntityType.OAK_BOAT,",
            "OAK_BOAT = buildBoat(entityBase, boatType,",
        ),
        (
            "HUSK = VanillaEntityType.inherited(ZOMBIE.factory(), ZOMBIE)",
            "HUSK = makeHusk(); Object unused = VanillaEntityType.inherited(ZOMBIE.factory(), ZOMBIE)",
        ),
        (
            ".type(EntityType.HUSK)\n                .build();",
            ".type(EntityType.HUSK);",
        ),
    ] {
        assert!(VANILLA.contains(broken), "fixture lacks {broken}");
        let source = VANILLA.replacen(broken, what, 1);
        assert!(pairs(&source).is_err(), "accepted {what}");
    }
}

#[test]
fn every_declared_type_must_be_parsed_once() {
    let unassigned = VANILLA.replacen(
        "public static final VanillaEntityType<CowEntity> COW;",
        "public static final VanillaEntityType<CowEntity> COW;\n    public static final VanillaEntityType<PigEntity> PIG;",
        1,
    );
    assert!(pairs(&unassigned).is_err());
    let twice = VANILLA.replacen(
        "HUSK = VanillaEntityType.inherited(ZOMBIE.factory(), ZOMBIE)\n                .type(EntityType.HUSK)",
        "HUSK = VanillaEntityType.inherited(ZOMBIE.factory(), ZOMBIE)\n                .type(EntityType.COW)",
        1,
    );
    assert!(pairs(&twice).is_err());
}

#[test]
fn an_unparseable_definition_fails_the_generator() {
    let indirect = DEFINITIONS.replacen("Identifier.of(\"boat\")", "Identifier.of(BOAT_NAME)", 1);
    assert!(entity_pairs(VANILLA, &indirect).is_err());
    let unassigned = DEFINITIONS.replacen(
        "        SPLASH = BedrockEntityDefinition.ofVanilla(Identifier.of(\"custom\", \"splash\"));\n",
        "",
        1,
    );
    assert!(entity_pairs(VANILLA, &unassigned).is_err());
}
