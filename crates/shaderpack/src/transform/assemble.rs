//! Merges a program's loose uniforms and writes each stage's final GLSL.

use super::StageDraft;
use crate::reflection::{
    LOOSE_UNIFORM_BINDING, LOOSE_UNIFORM_BLOCK, SHADOW_SAMPLER_BINDING, SHADOW_SAMPLER_NAME,
    TEXTURE_GROUP, UNIFORM_GROUP,
};
use crate::token::{Emitter, Tok};

/// The union of the stages' loose uniforms in first-declaration order. A name declared with
/// different types in two stages is an error.
pub(crate) fn merge_uniforms<'a>(
    drafts: impl IntoIterator<Item = &'a StageDraft>,
) -> Result<Vec<UboMember>, String> {
    let mut merged: Vec<UboMember> = vec![];
    for d in drafts {
        for m in &d.ubo {
            match merged.iter().find(|x| x.name == m.name) {
                Some(x) if x.spelling() != m.spelling() => {
                    return Err(format!(
                        "uniform `{}` is declared as `{}` and `{}` in different stages",
                        m.name,
                        x.spelling(),
                        m.spelling()
                    ));
                }
                Some(_) => {}
                None => merged.push(m.clone()),
            }
        }
    }
    Ok(merged)
}

/// GLSL text of a draft with the program's merged uniform block, and the include-expanded
/// source line of every output line (0 for generated lines).
pub(crate) fn assemble(
    draft: &StageDraft,
    ubo: &[UboMember],
    locations: &str,
) -> (String, Vec<u32>) {
    let mut e = Emitter::default();
    e.raw(&draft.header);
    e.raw(locations);
    e.tokens(&draft.hoisted);
    if ubo.iter().any(|m| m.name == "iris_Fog") {
        e.raw(FOG_STRUCT);
    }
    if !ubo.is_empty() {
        e.raw(&format!(
            "layout(std140, set = {UNIFORM_GROUP}, binding = {LOOSE_UNIFORM_BINDING}) uniform {LOOSE_UNIFORM_BLOCK} {{\n"
        ));
        for m in ubo {
            e.tokens(&m.toks);
        }
        e.raw("};\n");
    }
    e.tokens(&draft.resources);
    for class in &draft.samplers.filtering {
        e.raw(&format!(
            "layout(set = {TEXTURE_GROUP}, binding = {}) uniform sampler {};\n",
            class.binding(),
            class.glsl_name()
        ));
    }
    if draft.samplers.comparison {
        e.raw(&format!(
            "layout(set = {TEXTURE_GROUP}, binding = {SHADOW_SAMPLER_BINDING}) uniform samplerShadow {SHADOW_SAMPLER_NAME};\n"
        ));
    }
    e.raw(&draft.builtin_io);
    e.tokens(&draft.io);
    e.tokens(&draft.split_globals);
    e.tokens(&draft.body);
    e.finish()
}
/// Fog state of the compatibility profile, used by `gl_Fog`.
const FOG_STRUCT: &str = "struct iris_FogParameters { vec4 color; float density; float start; float end; float scale; };\n";

/// One member of the loose-uniform block: its name and full declaration tokens.
#[derive(Clone, Debug)]
pub(crate) struct UboMember {
    pub name: String,
    toks: Vec<Tok>,
}

impl UboMember {
    /// Creates a named loose uniform from its complete declaration.
    pub(super) fn new(name: &str, toks: Vec<Tok>) -> UboMember {
        UboMember {
            name: name.to_string(),
            toks,
        }
    }

    /// Returns the declaration spelling used to compare stage uniforms.
    fn spelling(&self) -> String {
        self.toks
            .iter()
            .map(Tok::text)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reflection::SamplerClass;
    use crate::token::lex;
    use crate::transform::body::SamplerUse;
    use std::collections::BTreeSet;

    /// Creates a stage fixture with the supplied uniforms, body and sampler usage.
    fn draft(ubo: &[(&str, &str)], body: &str, samplers: SamplerUse) -> StageDraft {
        StageDraft {
            header: "#version 450\n".to_owned(),
            hoisted: vec![],
            ubo: ubo
                .iter()
                .map(|(name, decl)| UboMember::new(name, lex(decl)))
                .collect(),
            resources: vec![],
            samplers,
            builtin_io: String::new(),
            io: vec![],
            split_globals: vec![],
            body: lex(body),
            vertex_inputs: vec![],
            frag_outputs: vec![],
            resource_names: vec![],
        }
    }

    #[test]
    fn stages_share_one_merged_uniform_block() {
        let vertex = draft(
            &[("frameTimeCounter", "float frameTimeCounter;")],
            "void main() {}",
            SamplerUse::default(),
        );
        let fragment = draft(
            &[
                ("viewWidth", "float viewWidth;"),
                ("frameTimeCounter", "float frameTimeCounter;"),
            ],
            "void main() {}",
            SamplerUse {
                filtering: BTreeSet::from([SamplerClass::RepeatLinear, SamplerClass::ClampNearest]),
                comparison: false,
            },
        );
        let ubo = merge_uniforms([&vertex, &fragment]).unwrap();
        let names: Vec<_> = ubo.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["frameTimeCounter", "viewWidth"]);

        let (glsl, origins) = assemble(&fragment, &ubo, "");
        assert!(glsl.contains(&format!(
            "uniform {LOOSE_UNIFORM_BLOCK} {{\nfloat frameTimeCounter;\nfloat viewWidth;\n}};"
        )));
        // One sampler per class the stage reads, at the class's binding.
        for class in [SamplerClass::RepeatLinear, SamplerClass::ClampNearest] {
            assert!(glsl.contains(&format!(
                "layout(set = {TEXTURE_GROUP}, binding = {}) uniform sampler {};",
                class.binding(),
                class.glsl_name()
            )));
        }
        assert!(!glsl.contains(SamplerClass::ClampLinear.glsl_name()));
        assert!(!glsl.contains(SHADOW_SAMPLER_NAME));
        assert_eq!(origins.len(), glsl.lines().count());

        let clash = draft(
            &[("viewWidth", "int viewWidth;")],
            "",
            SamplerUse::default(),
        );
        assert!(merge_uniforms([&fragment, &clash]).is_err());
    }
}
