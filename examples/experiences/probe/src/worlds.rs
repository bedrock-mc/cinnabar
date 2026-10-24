//! Server WIT world and scoped-state probes, each using the real guest imports.
use experience_sdk::server::*;

/// Declares a mod-owned empty-world definition without opening terrain in unrelated tests.
pub(super) fn definitions() -> WorldRegistration {
    WorldRegistration {
        definitions: vec![WorldDefinition {
            id: WorldDefinitionId {
                value: "probe:plain".into(),
            },
            settings: WorldSettings {
                generator: Generator::Empty,
                dimension: DimensionType::Overworld,
                persistent: false,
            },
        }],
        startup: vec![],
    }
}

/// Stages creation, travel, unloading, or three typed-scope writes for an interaction.
pub(super) fn interact(ctx: &Callback, player: &str, action: i32) -> Result<(), GuestError> {
    let refused = |error: WorldError| GuestError::Rejected(format!("{error:?}"));
    let id = WorldInstanceId {
        value: if matches!(action, 34 | 35) {
            "probe:entities"
        } else {
            "probe:one"
        }
        .into(),
    };
    match action {
        30 | 34 => ctx
            .create_world(&WorldInstance {
                id,
                definition: WorldDefinitionId {
                    value: "probe:plain".into(),
                },
            })
            .map_err(refused)?,
        31 | 35 => {
            let handle = ctx.resolve_world(&id).map_err(refused)?;
            ctx.transfer(
                player,
                &handle,
                Position {
                    x: 1.25,
                    y: 70.5,
                    z: 3.75,
                },
            )
            .map_err(refused)?;
        }
        32 => {
            let handle = ctx.resolve_world(&id).map_err(refused)?;
            ctx.unload_world(&handle).map_err(refused)?;
        }
        33 => {
            let location = ctx.player_location(player).map_err(refused)?;
            let current = ctx.world();
            if location.world.id.value != current.id.value
                || location.world.generation != current.generation
            {
                return Err(GuestError::Rejected(
                    "actor and callback world disagree".into(),
                ));
            }
            for scope in [StoreScope::Player, StoreScope::Instance, StoreScope::Mod] {
                ctx.store_write(StoreKey { scope, schema: 1 }, Some(&[1, 2]))
                    .map_err(refused)?;
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}

/// Exercises the player export and actor position import without granting access to another player.
pub(super) fn on_player(
    ctx: &Callback,
    player: PlayerId,
    event: PlayerEvent,
) -> Result<(), GuestError> {
    let kind = match event {
        PlayerEvent::Join(_) => "join",
        PlayerEvent::Leave(_) => return Ok(()),
        PlayerEvent::WorldChange(_) => "world_change",
        PlayerEvent::Move(_) => "move",
    };
    let refused = |error: WorldError| GuestError::Rejected(format!("{error:?}"));
    let location = ctx.player_location(&player).map_err(refused)?;
    ctx.tell(&player, &format!("player {kind} {}", location.epoch))
        .map_err(refused)
}
