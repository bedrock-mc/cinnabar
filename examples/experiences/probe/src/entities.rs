//! Guest conformance cases for entity registration, staged movement and steering.
use experience_sdk::server::*;

/// Returns the fixture's one registered entity identity.
fn ship() -> EntityTypeId {
    EntityTypeId {
        value: "probe:ship".into(),
    }
}

/// Declares a renderer reuse, a controlling seat and one synchronized property.
pub fn definitions() -> Vec<EntityDefinition> {
    vec![EntityDefinition {
        id: ship(),
        collision: EntityBox {
            min: Position {
                x: -0.5,
                y: 0.0,
                z: -0.5,
            },
            max: Position {
                x: 0.5,
                y: 1.0,
                z: 0.5,
            },
        },
        appearance: EntityAppearance::Vanilla(EntityTypeId {
            value: "minecraft:boat".into(),
        }),
        seats: vec![EntitySeat {
            position: Position {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            },
            rotation_limit: Some(0.0),
            steers: true,
        }],
        properties: vec![EntityPropertyDef {
            id: EntityPropertyId {
                value: "probe:throttle".into(),
            },
            min: -1.0,
            max: 1.0,
            initial: 0.0,
        }],
    }]
}

/// Exercises entity operations only in the dedicated conformance instance.
pub fn on_tick(ctx: &Callback) -> Result<(), GuestError> {
    if ctx.world().id.value != "probe:entities" {
        return Ok(());
    }
    if ctx.entities().is_empty() {
        ctx.spawn_entity(
            &ship(),
            Position {
                x: 0.0,
                y: 70.0,
                z: 0.0,
            },
            EntityRotation {
                yaw: 0.0,
                pitch: 0.0,
            },
        )
        .map_err(failed)?;
    }
    for mut state in ctx.entities() {
        state.position.x += 0.25;
        ctx.update_entity(&state, false).map_err(failed)?;
    }
    Ok(())
}

/// Boards the actor on interaction and applies steering to authoritative velocity.
pub fn on_entity(
    ctx: &Callback,
    handle: EntityHandle,
    event: EntityEvent,
) -> Result<(), GuestError> {
    match event {
        EntityEvent::Interact(_) => ctx.mount_entity(handle, 0).map_err(failed)?,
        EntityEvent::Steering(input) => {
            let Some(mut state) = ctx
                .entities()
                .into_iter()
                .find(|s| s.handle.id == handle.id && s.handle.generation == handle.generation)
            else {
                return Err(GuestError::Failed("missing entity".into()));
            };
            state.velocity.x = f64::from(input.sideways);
            state.velocity.z = f64::from(input.forward);
            ctx.update_entity(&state, false).map_err(failed)?;
        }
        _ => {}
    };
    Ok(())
}

/// Converts a host refusal into the probe's explicit guest failure.
fn failed(error: WorldError) -> GuestError {
    GuestError::Failed(error.to_string())
}
