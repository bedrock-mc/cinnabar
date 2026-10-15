//! Controller initialization programs share emitter-local values by their compiled names.
use crate::molang::{Interner, Program, Queries, Rng};

#[derive(Clone, Debug)]
pub struct ActorInitialization {
    program: Program,
    names: Interner,
    queries: Box<[Box<str>]>,
}

impl ActorInitialization {
    /// Compiles a bounded script once when its controller pack is admitted.
    pub fn compile(source: &str) -> Option<Self> {
        if source.len() > 4096 {
            return None;
        }
        let mut names = Interner::with_builtins();
        let (program, queries) = Program::parse_with_actor_queries(source, &mut names)?;
        Some(Self {
            program,
            names,
            queries,
        })
    }

    /// Named actor queries the host must sample without inheriting owner variables.
    pub fn query_names(&self) -> impl Iterator<Item = &str> {
        self.queries.iter().map(AsRef::as_ref)
    }

    /// Evaluates once after emitter creation; matching names read and write the emitter's own map.
    pub(crate) fn apply(
        &self,
        interner: &Interner,
        vars: &mut [f32],
        rng: &mut Rng,
        queries: &Queries,
    ) {
        let mut script_vars = vec![f32::NAN; self.names.len()];
        for (name, slot) in self.names.named_slots() {
            if let Some(emitter_slot) = interner.get(name) {
                script_vars[slot as usize] = vars[emitter_slot as usize];
            }
        }
        self.program.eval(&mut script_vars, rng, queries);
        for (name, slot) in self.names.named_slots() {
            if let Some(emitter_slot) = interner.get(name) {
                vars[emitter_slot as usize] = script_vars[slot as usize];
            }
        }
    }
}
