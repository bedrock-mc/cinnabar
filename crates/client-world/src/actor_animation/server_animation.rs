//! Packet-selected animation states share the actor's clocks and script budget.
use super::{tick::WeightedClip, *};

/// Compiles a packet expression outside the world owner, receiving its declared version.
pub type ServerAnimationCompiler = fn(&str, i32) -> Option<assets::MolangProgram>;

#[derive(Debug)]
pub(super) struct StopCode {
    program: assets::MolangProgram,
    layout: VariableLayout,
}

#[derive(Debug)]
pub(crate) struct Request {
    animation: Arc<str>,
    controller: Arc<str>,
    next_state: Arc<str>,
    stop: Option<Arc<StopCode>>,
    blend_out: f32,
}

#[derive(Clone, Debug)]
struct Definition {
    request: Arc<Request>,
    symbol: u32,
}

#[derive(Clone, Debug)]
struct Playing {
    definition: usize,
    started: u64,
    variables: MolangVariables,
}

#[derive(Clone, Debug)]
struct Outgoing {
    playing: Playing,
    changed: u64,
    duration: f32,
}

#[derive(Clone, Debug)]
pub(super) struct Controller {
    name: Arc<str>,
    definitions: Vec<Definition>,
    current: Option<Playing>,
    outgoing: Option<Outgoing>,
}

impl ActorAnimationStore {
    pub(crate) fn prepare_server_animation(
        &mut self,
        action: &protocol::ActorActionEvent,
    ) -> Option<Arc<Request>> {
        let protocol::ActorActionKind::Custom {
            animation,
            controller,
            next_state,
            stop_expression,
            stop_expression_version,
        } = &action.kind
        else {
            return None;
        };
        let stop = if stop_expression.is_empty() {
            None
        } else {
            let compiled = self
                .server_compiler
                .and_then(|compile| compile(stop_expression, *stop_expression_version));
            let Some(program) = compiled else {
                self.stats.invalid_server_stop_expressions =
                    self.stats.invalid_server_stop_expressions.saturating_add(1);
                return None;
            };
            let layout = VariableLayout::from_symbols(&program.symbols);
            Some(Arc::new(StopCode { program, layout }))
        };
        Some(Arc::new(Request {
            animation: Arc::clone(animation),
            controller: Arc::clone(controller),
            next_state: Arc::clone(next_state),
            stop,
            blend_out: if action.data.is_finite() {
                action.data.max(0.0)
            } else {
                0.0
            },
        }))
    }

    pub(crate) fn start_server_animation(&mut self, runtime_id: u64, request: Arc<Request>) {
        let Some(lifetime) = self.runtime_to_lifetime.get(&runtime_id) else {
            return;
        };
        let Some(state) = self.rigs.get_mut(lifetime) else {
            return;
        };
        let catalog = if state.pack {
            self.pack.as_ref().map(|pack| &pack.assets)
        } else {
            self.assets.as_ref()
        };
        let Some(assets) = catalog else {
            return;
        };
        let Some(clip) = assets.server_animation_clip(state.geometry_binding, &request.animation)
        else {
            return;
        };
        let Some(symbol) = assets
            .animation_clips()
            .get(clip as usize)
            .map(|clip| clip.symbol)
        else {
            return;
        };
        let count: usize = state
            .server_animations
            .iter()
            .map(|controller| controller.definitions.len())
            .sum();
        let controller = match state
            .server_animations
            .iter()
            .position(|controller| controller.name == request.controller)
        {
            Some(index) => &mut state.server_animations[index],
            None if count < MAX_ACTOR_ACTION_HISTORY => {
                state.server_animations.push(Controller {
                    name: Arc::clone(&request.controller),
                    definitions: Vec::new(),
                    current: None,
                    outgoing: None,
                });
                state.server_animations.last_mut().unwrap()
            }
            None => return,
        };
        let definition = match controller
            .definitions
            .iter()
            .position(|definition| definition.request.animation == request.animation)
        {
            Some(index) => {
                controller.definitions[index] = Definition { request, symbol };
                index
            }
            None if count < MAX_ACTOR_ACTION_HISTORY => {
                controller.definitions.push(Definition { request, symbol });
                controller.definitions.len() - 1
            }
            None => return,
        };
        let now = self.completed_tick.saturating_sub(state.lifetime_epoch);
        controller.change(Some(definition), now, runtime_id);
        if state.creeper {
            state.swell_sampling = Some(super::render_frame::swell::SwellSampling::new(
                assets,
                state.rig_binding,
                state.geometry_binding,
                &state.controllers,
                &dependency_clips(assets, state.geometry_binding, &state.server_animations),
            ));
        }
        if let Some(ui) = state.ui_animation.as_mut() {
            ui.server_animations.clone_from(&state.server_animations);
        }
        state.replay = None;
    }
}

impl Controller {
    fn change(&mut self, definition: Option<usize>, now: u64, seed: u64) {
        self.outgoing = self.current.take().and_then(|playing| {
            let duration = self.definitions[playing.definition].request.blend_out;
            (duration > 0.0).then_some(Outgoing {
                playing,
                changed: now,
                duration,
            })
        });
        self.current = definition.map(|definition| Playing {
            definition,
            started: now,
            variables: self.definitions[definition]
                .request
                .stop
                .as_ref()
                .map_or_else(MolangVariables::default, |stop| stop.layout.fresh(seed)),
        });
    }
}

pub(super) struct SelectionInput<'a> {
    pub(super) geometry: usize,
    pub(super) clocks: &'a clock::ClipClocks,
    pub(super) advance: bool,
}

pub(super) fn select(
    evaluator: &Evaluator<'_>,
    variables: &mut MolangVariables,
    controllers: &mut [Controller],
    input: SelectionInput<'_>,
    clips: &mut Vec<WeightedClip>,
    budget: &mut EvalBudget<'_>,
) -> Result<(), EvalError> {
    let SelectionInput {
        geometry,
        clocks,
        advance,
    } = input;
    for controller in controllers {
        budget.charge_work()?;
        if advance {
            let transition = if let Some(playing) = controller.current.as_mut() {
                let definition = &controller.definitions[playing.definition];
                if let Some(stop) = definition.request.stop.as_ref() {
                    let clip_index = resolve(evaluator.assets, geometry, definition.symbol)?;
                    let clip = &evaluator.assets.animation_clips()[clip_index];
                    let elapsed = elapsed(evaluator.life_tick, playing.started);
                    let clock = clocks.get(&(clip_index, playing.started, clock::Basis::Lifetime));
                    let finished =
                        clock.map_or(elapsed >= clip.length_seconds.get(), |clock| clock.finished);
                    let time = clock.map_or(elapsed, |clock| clock.time);
                    playing.variables.copy_named_from(
                        &stop.program.symbols,
                        evaluator.assets.molang_symbols(),
                        variables,
                    );
                    playing.variables.inherit_write_capture(variables);
                    playing.variables.clear_temporaries();
                    let stop_evaluator = Evaluator {
                        layout: &stop.layout,
                        program: Some(&stop.program),
                        anim_tick: evaluator.life_tick.saturating_sub(playing.started),
                        anim_time: Some(time),
                        finished: (finished, finished),
                        ..*evaluator
                    };
                    let stopped = stop_evaluator
                        .run(0, &mut playing.variables, 0.0, budget)?
                        .truthy();
                    variables.copy_named_from(
                        evaluator.assets.molang_symbols(),
                        &stop.program.symbols,
                        &playing.variables,
                    );
                    stopped.then(|| Arc::clone(&definition.request.next_state))
                } else {
                    None
                }
            } else {
                None
            };
            if let Some(next) = transition
                && budget.take_transition()
            {
                let target = controller
                    .definitions
                    .iter()
                    .position(|definition| definition.request.animation == next);
                controller.change(target, evaluator.life_tick, evaluator.actor.runtime_id);
            }
        }
        let amount = controller.outgoing.as_ref().map_or(1.0, |outgoing| {
            (elapsed(evaluator.life_tick, outgoing.changed) / outgoing.duration).clamp(0.0, 1.0)
        });
        if let Some(outgoing) = controller.outgoing.as_ref() {
            append(
                evaluator.assets,
                geometry,
                &controller.definitions[outgoing.playing.definition],
                &outgoing.playing,
                1.0 - amount,
                clips,
            )?;
        }
        if let Some(playing) = controller.current.as_ref() {
            append(
                evaluator.assets,
                geometry,
                &controller.definitions[playing.definition],
                playing,
                amount,
                clips,
            )?;
        }
        if advance && amount >= 1.0 {
            controller.outgoing = None;
        }
    }
    Ok(())
}

/// Packet definitions can select clips outside the entity's initially bound animation graph.
pub(super) fn dependency_clips(
    assets: &RuntimeEntityAssets,
    geometry: usize,
    controllers: &[Controller],
) -> Vec<usize> {
    controllers
        .iter()
        .flat_map(|controller| &controller.definitions)
        .filter_map(|definition| resolve(assets, geometry, definition.symbol).ok())
        .collect()
}

fn elapsed(now: u64, started: u64) -> f32 {
    now.saturating_sub(started) as f32 * ANIMATION_TICK_SECONDS
}

fn resolve(assets: &RuntimeEntityAssets, geometry: usize, symbol: u32) -> Result<usize, EvalError> {
    let geometry = assets
        .rig_geometries()
        .get(geometry)
        .ok_or(EvalError::Invalid)?
        .geometry;
    assets
        .clip_for_geometry(symbol, geometry)
        .map(|clip| clip as usize)
        .ok_or(EvalError::Invalid)
}

fn append(
    assets: &RuntimeEntityAssets,
    geometry: usize,
    definition: &Definition,
    playing: &Playing,
    weight: f32,
    clips: &mut Vec<WeightedClip>,
) -> Result<(), EvalError> {
    if weight > f32::EPSILON {
        clips.push(WeightedClip {
            clip: resolve(assets, geometry, definition.symbol)?,
            weight,
            started_tick: playing.started,
            clock: clock::Basis::Lifetime,
            time: 0.0,
            blend: None,
        });
    }
    Ok(())
}
