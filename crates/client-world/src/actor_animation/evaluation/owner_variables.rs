//! Owner variables and query inputs inherited by equipment.
use super::*;

impl<'a> ActorAnimationVariables<'a> {
    /// Borrows completed script values from the owner's asset catalog.
    pub(in crate::actor_animation) fn new(
        assets: Option<&'a RuntimeEntityAssets>,
        variables: &'a MolangVariables,
        life_tick: u64,
    ) -> Self {
        Self {
            assets,
            variables: Some(variables),
            life_tick,
            input: None,
            item_context: None,
            complete_spear: false,
        }
    }

    /// Equipment observes the same fixed-tick movement and swim blend as its owner.
    pub(in crate::actor_animation) fn with_input(mut self, input: Option<ActorTickInput>) -> Self {
        self.input = input;
        self
    }

    /// Retains item component facts and whether the player needs native spear pose completion.
    pub(in crate::actor_animation) fn with_item_context(
        mut self,
        context: Option<&'a ActorTickContext>,
        complete_spear: bool,
    ) -> Self {
        self.item_context = context;
        self.complete_spear = complete_spear;
        self
    }

    /// Selected owner component facts also serve owning-entity queries on held attachables.
    pub(in crate::actor_animation) fn item_timings(
        self,
    ) -> (Option<protocol::KineticWeaponTiming>, Option<f32>) {
        self.item_context.map_or((None, None), |context| {
            (context.main_hand_kinetic, context.main_hand_swing_seconds)
        })
    }

    /// The owning item's explicit tag stays available to attachable query contexts.
    pub(in crate::actor_animation) fn is_spear(self) -> bool {
        self.item_context
            .is_some_and(|context| context.main_hand_is_spear)
    }

    /// Samples the owner-provided spear variables at the held item's frame fraction.
    pub(in crate::actor_animation) fn sample_spear_to(
        self,
        slots: &super::spear::Slots,
        output: &mut MolangVariables,
        actor: &ActorSnapshot,
        input: &ActorTickInput,
        alpha: f32,
    ) {
        if self.complete_spear
            && let Some(context) = self.item_context
        {
            let mut context = context.clone();
            context.frame_alpha = alpha;
            slots.apply(output, actor, &context, input, input.attack_time);
        }
    }

    /// Retains query inputs as well as script variables for worn animation clips.
    pub(in crate::actor_animation) fn input(self) -> Option<ActorTickInput> {
        self.input
    }

    /// Returns elapsed fixed ticks for this owner lifetime.
    pub(in crate::actor_animation) fn life_tick(self) -> u64 {
        self.life_tick
    }

    /// Borrows one inherited value without constructing another variable layout.
    pub(in crate::actor_animation) fn value(self, name: &str) -> Option<&'a MolangValue> {
        let symbols = self.assets?.molang_symbols();
        let first = symbols.partition_point(|symbol| symbol.kind < MolangSymbolKind::Variable);
        let end = symbols.partition_point(|symbol| symbol.kind <= MolangSymbolKind::Variable);
        let slot = symbols[first..end]
            .binary_search_by(|symbol| symbol.identifier.as_ref().cmp(name))
            .ok()?;
        self.variables?.values.get(slot)?.as_ref()
    }

    /// The catalog that owns these retained rig script values.
    pub(in crate::actor_animation) fn asset_catalog(self) -> Option<&'a RuntimeEntityAssets> {
        self.assets
    }

    /// Copies declared owner variables into the attachable catalog by name.
    pub(in crate::actor_animation) fn copy_to(
        self,
        assets: &RuntimeEntityAssets,
        layout: &VariableLayout,
        output: &mut MolangVariables,
    ) {
        let (Some(owner_assets), Some(variables)) = (self.assets, self.variables) else {
            return;
        };
        let owner_symbols = owner_assets.molang_symbols();
        let first =
            owner_symbols.partition_point(|symbol| symbol.kind < MolangSymbolKind::Variable);
        for (offset, value) in variables.values.iter().enumerate() {
            let Some(value) = value else {
                continue;
            };
            let Some(symbol) = owner_symbols.get(first + offset) else {
                break;
            };
            if let Some(slot) = layout.named_slot(assets, &symbol.identifier) {
                output.values[slot] = Some(value.clone());
            }
        }
    }
}
