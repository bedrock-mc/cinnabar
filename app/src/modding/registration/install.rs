//! Independent instance transitions and stable presentation ownership on the frame thread.

use super::*;
use presentation::{clear_presentation, refresh_presentation};

/// Revokes removed or changed grants before input, independently of loader progress.
pub(super) fn sync_authority(world: &mut World) {
    let Some(watcher) = world.get_resource::<Watcher>() else {
        return;
    };
    let desired = watcher.desired.lock().expect("mod desired list");
    let enabled: Vec<_> = if watcher.alive.load(Ordering::Acquire) {
        desired
            .registration
            .as_ref()
            .into_iter()
            .flat_map(|registration| registration.mods.iter())
            .filter(|entry| entry.enabled)
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    drop(desired);
    if let Some(mut runtime) = world.get_resource_mut::<ModRuntime>() {
        for index in 0..runtime.metadata.len() {
            let Some(registered) = &runtime.metadata[index].registration else {
                continue;
            };
            let authorized = enabled.iter().any(|entry| {
                entry.component == registered.entry.component
                    && entry.grants == registered.entry.grants
            });
            if !authorized {
                runtime.hosts.host_mut(index).revoke();
            }
        }
    }
    let retired = world
        .get_resource_mut::<crate::modding::packets::PacketMods>()
        .map(|mut packets| packets.authorize(&enabled))
        .unwrap_or_default();
    for program in retired {
        world.resource::<Watcher>().retire(Retired::Packet(program));
    }
    world.resource::<Watcher>().remember_settings(
        world.get_resource::<ModRuntime>(),
        world.get_resource::<UiRuntimeResource>().map(|ui| &ui.0),
    );
    refresh_presentation(world);
}

/// Removes one instance while leaving every other host and its stable panel owner intact.
fn detach(world: &mut World, index: usize) -> ModHost {
    let owner = world.resource::<ModRuntime>().metadata[index].owner;
    if let Some(mut presentation) = world.get_resource_mut::<UiPresentationResource>() {
        presentation.mod_ui_mut().set_prepared_panel_ui(owner, None);
        let _ = presentation.mod_ui_mut().set_mod_panel(owner, None);
    }
    if world.resource::<ModRuntime>().hosts.host_count() == 1 {
        let runtime = world
            .remove_resource::<ModRuntime>()
            .expect("last mod runtime");
        world.remove_resource::<VisualTimeOverride>();
        world.remove_resource::<ModInteraction>();
        clear_presentation(world);
        return runtime.hosts.into_single();
    }
    let mut runtime = world.resource_mut::<ModRuntime>();
    runtime.remove_host(index)
}

/// Finds only the host belonging to this registered component; no frame filesystem access.
fn index(world: &World, component: &std::path::Path) -> Option<usize> {
    world
        .get_resource::<ModRuntime>()?
        .metadata
        .iter()
        .position(|metadata| {
            metadata
                .registration
                .as_ref()
                .is_some_and(|registered| registered.entry.component == component)
        })
}

/// Treats reordered or delayed results as idempotent once the same source and fence are running.
fn already_admitted(world: &World, candidate: &Candidate) -> bool {
    let Some(index) = index(world, &candidate.registration.component) else {
        return false;
    };
    let runtime = world.resource::<ModRuntime>();
    let host = runtime.hosts.host(index);
    host.is_active()
        && Some(host.drawing_session()) == candidate.frame_host().map(ModHost::drawing_session)
        && runtime.metadata[index]
            .registration
            .as_ref()
            .is_some_and(|registered| registered.identity == candidate.identity)
}

/// Installs a prepared candidate without changing the other instances or their preferences.
fn publish(world: &mut World, candidate: Candidate) -> Result<(), Box<Candidate>> {
    if already_admitted(world, &candidate) {
        candidate.retire(world.resource::<Watcher>());
        return Ok(());
    }
    if matches!(&candidate.runtime, CandidateRuntime::Packet(_)) {
        if let Some(previous) = index(world, &candidate.registration.component) {
            let old = detach(world, previous);
            world.resource::<Watcher>().retire(old);
        }
        let CandidateRuntime::Packet(program) = candidate.runtime else {
            unreachable!()
        };
        crate::modding::packets::publish(
            world,
            candidate.registration,
            candidate.identity,
            program,
        );
        return Ok(());
    }
    let previous = index(world, &candidate.registration.component);
    if previous.is_none()
        && world
            .get_resource::<ModRuntime>()
            .is_some_and(|runtime| runtime.hosts.host_count() >= mod_api::MAX_LOADED_MODS)
    {
        return Err(Box::new(candidate));
    }
    if previous.is_some_and(|index| {
        super::super::admission::DrawingContext::capture(
            world.resource::<ModRuntime>().hosts.host(index),
            world.get_resource::<UiRuntimeResource>().map(|ui| &ui.0),
        ) != candidate.drawing
    }) {
        return Err(Box::new(candidate));
    }
    let Candidate {
        runtime: CandidateRuntime::Frame(prepared),
        registration,
        identity,
        ..
    } = candidate
    else {
        unreachable!()
    };
    let mut prepared = *prepared;
    let owner =
        mod_ui::PanelUiOwner::Registration(selection::component_id(&registration.component));
    let old_packet = world
        .get_resource_mut::<crate::modding::packets::PacketMods>()
        .and_then(|mut packets| packets.remove(&registration.component));
    if let Some(program) = old_packet {
        world.resource::<Watcher>().retire(Retired::Packet(program));
    }
    let mut old = previous.map(|index| detach(world, index));
    prepared.host.activate_settings(old.as_mut());
    if let Some(mut presentation) = world.get_resource_mut::<UiPresentationResource>() {
        presentation
            .mod_ui_mut()
            .set_prepared_panel_ui(owner, prepared.panel_ui);
    }
    let metadata = HostMetadata {
        font: prepared.font,
        branding: prepared.branding,
        owner,
        registration: Some(RegisteredHost {
            entry: registration,
            identity,
        }),
    };
    if let Some(mut runtime) = world.get_resource_mut::<ModRuntime>() {
        runtime.push_host(prepared.host, metadata);
    } else {
        world.insert_resource(VisualTimeOverride(None));
        world.insert_resource(ModInteraction::default());
        world.insert_resource(ModRuntime::new(prepared.host, metadata));
    }
    if let Some(old) = old {
        world.resource::<Watcher>().retire(old);
    }
    Ok(())
}

/// Orders existing instances without re-instantiating guests or losing their panel selection.
fn reorder(world: &mut World, entries: &[Entry]) {
    let Some(mut runtime) = world.get_resource_mut::<ModRuntime>() else {
        return;
    };
    for (position, entry) in entries.iter().filter(|entry| entry.enabled).enumerate() {
        if let Some(index) = runtime.metadata.iter().position(|metadata| {
            metadata
                .registration
                .as_ref()
                .is_some_and(|registered| registered.entry.component == entry.component)
        }) {
            // Failed entries have no host, so only preceding accepted entries consume positions.
            let destination = entries
                .iter()
                .filter(|entry| entry.enabled)
                .take(position)
                .filter(|entry| {
                    runtime.metadata.iter().any(|metadata| {
                        metadata
                            .registration
                            .as_ref()
                            .is_some_and(|registered| registered.entry.component == entry.component)
                    })
                })
                .count();
            for next in (destination..index).rev() {
                runtime.swap_hosts(next, next + 1);
            }
        }
    }
}

/// Applies a current batch, retaining the previous instance on failed reloads.
pub(super) fn apply(world: &mut World, update: Update) {
    let watcher = world.resource::<Watcher>();
    let authority = watcher.desired.clone();
    let desired = authority.lock().expect("mod desired list");
    let current = desired.generation == update.generation && watcher.alive.load(Ordering::Acquire);
    let candidates = update
        .changes
        .iter()
        .filter(|change| change.result.is_ok())
        .count();
    let managed = world.get_resource::<ModRuntime>().map_or(0, |runtime| {
        runtime
            .metadata
            .iter()
            .filter(|metadata| metadata.registration.is_some())
            .count()
    });
    if watcher.retirement_used() + candidates + managed > watcher.retired.capacity().unwrap_or(0) {
        *watcher.pending.lock().expect("mod pending update") = Some(update);
        return;
    }
    if !current {
        for change in update.changes {
            if let Ok(candidate) = change.result {
                candidate.retire(watcher);
            }
        }
        return;
    }
    let registration = &update.registration;
    let changes = &update.changes;
    let inactive: Vec<_> = registration
        .mods
        .iter()
        .filter(|entry| entry.enabled)
        .filter(|entry| {
            !changes
                .iter()
                .any(|change| change.entry.component == entry.component)
        })
        .filter(|entry| {
            if world
                .get_resource::<crate::modding::packets::PacketMods>()
                .is_some_and(|packets| packets.admitted(&entry.component))
            {
                return false;
            }
            index(world, &entry.component).is_none_or(|index| {
                world
                    .resource::<ModRuntime>()
                    .hosts
                    .host(index)
                    .is_revoked()
            })
        })
        .map(|entry| entry.component.clone())
        .collect();
    if !inactive.is_empty() {
        for change in update.changes {
            if let Ok(candidate) = change.result {
                candidate.retire(watcher);
            }
        }
        watcher.send(Message::Retry {
            generation: update.generation,
            inactive,
        });
        return;
    }
    let stale_settings = update
        .changes
        .iter()
        .filter_map(|change| change.result.as_ref().ok())
        .filter(|candidate| !already_admitted(world, candidate))
        .any(|candidate| {
            index(world, &candidate.registration.component).is_some_and(|index| {
                let old = world.resource::<ModRuntime>().hosts.host(index);
                candidate.frame_host().is_some_and(|host| {
                    host.settings_path().is_some()
                        && host.settings_path() == old.settings_path()
                        && host.settings_seed() != old.settings_snapshot()
                })
            })
        });
    if stale_settings {
        watcher.remember_settings(
            world.get_resource::<ModRuntime>(),
            world.get_resource::<UiRuntimeResource>().map(|ui| &ui.0),
        );
        for change in update.changes {
            if let Ok(candidate) = change.result {
                candidate.retire(watcher);
            }
        }
        watcher.send(Message::Retry {
            generation: update.generation,
            inactive: Vec::new(),
        });
        return;
    }
    let removed: Vec<_> = world
        .get_resource::<ModRuntime>()
        .into_iter()
        .flat_map(|runtime| runtime.metadata.iter())
        .filter_map(|metadata| metadata.registration.as_ref())
        .filter(|registered| {
            !update
                .registration
                .mods
                .iter()
                .any(|entry| entry.enabled && entry.component == registered.entry.component)
        })
        .map(|registered| registered.entry.component.clone())
        .collect();
    for component in removed {
        if let Some(index) = index(world, &component) {
            let old = detach(world, index);
            world.resource::<Watcher>().retire(old);
        }
    }
    let retired_packets = world
        .get_resource_mut::<crate::modding::packets::PacketMods>()
        .map(|mut packets| packets.remove_disabled(&update.registration.mods))
        .unwrap_or_default();
    for program in retired_packets {
        world.resource::<Watcher>().retire(Retired::Packet(program));
    }
    let mut statuses = Vec::new();
    for change in update.changes {
        let component = change.entry.component;
        let reload = Some(change.entry.reload);
        let error = match change.result {
            Ok(candidate) => match publish(world, *candidate) {
                Ok(()) => {
                    let state = if world
                        .get_resource::<crate::modding::packets::PacketMods>()
                        .is_some_and(|packets| packets.admitted(&component))
                    {
                        "next-join"
                    } else {
                        "loaded"
                    };
                    statuses.push(Status::new(component, state, None, reload));
                    continue;
                }
                Err(candidate) => {
                    candidate.retire(world.resource::<Watcher>());
                    "local extension limit reached".into()
                }
            },
            Err(error) => error,
        };
        let running = index(world, &component)
            .is_some_and(|index| world.resource::<ModRuntime>().hosts.host(index).is_active())
            || world
                .get_resource::<crate::modding::packets::PacketMods>()
                .is_some_and(|packets| packets.admitted(&component));
        statuses.push(Status::new(
            component,
            if running { "reload-error" } else { "error" },
            Some(error),
            reload,
        ));
    }
    reorder(world, &update.registration.mods);
    let retired = world
        .get_resource_mut::<crate::modding::packets::PacketMods>()
        .map(|mut packets| packets.authorize(&update.registration.mods))
        .unwrap_or_default();
    for program in retired {
        world.resource::<Watcher>().retire(Retired::Packet(program));
    }
    refresh_presentation(world);
    // A trapped identity stays admitted but cannot execute. Only revoked identities need recovery.
    let mut installed: BTreeMap<_, _> = world
        .get_resource::<ModRuntime>()
        .into_iter()
        .flat_map(|runtime| {
            runtime
                .metadata
                .iter()
                .enumerate()
                .filter_map(move |(index, metadata)| {
                    let host = runtime.hosts.host(index);
                    let registered = metadata.registration.as_ref()?;
                    (!host.is_revoked()).then(|| {
                        (
                            registered.entry.component.clone(),
                            Admitted {
                                source: registered.identity,
                                drawing: host.drawing_session(),
                            },
                        )
                    })
                })
        })
        .collect();
    if let Some(packets) = world.get_resource::<crate::modding::packets::PacketMods>() {
        installed.extend(packets.identities().map(|(path, source)| {
            (
                path,
                Admitted {
                    source,
                    drawing: None,
                },
            )
        }));
    }
    world
        .resource::<Watcher>()
        .remember_companions(world.get_resource::<ModRuntime>());
    world
        .resource::<Watcher>()
        .applied
        .store(update.generation, Ordering::Release);
    world.resource::<Watcher>().send(Message::Acknowledge {
        generation: update.generation,
        request_id: update.registration.request_id,
        installed,
        statuses,
    });
}
