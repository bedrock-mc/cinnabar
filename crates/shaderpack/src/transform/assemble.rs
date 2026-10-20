//! Merges a program's loose uniforms and writes each stage's final GLSL.

use super::StageDraft;
use super::items::{parse_decl, split_items};
use crate::reflection::{
    LOOSE_UNIFORM_BINDING, LOOSE_UNIFORM_BLOCK, SHADOW_SAMPLER_BINDING, SHADOW_SAMPLER_NAME,
    TEXTURE_GROUP, UNIFORM_GROUP,
};
use crate::token::{Emitter, Tok};
use std::collections::{HashMap, HashSet};

/// The union of the stages' loose uniforms in first-declaration order. A name declared with
/// different types in two stages is an error.
pub(crate) fn merge_uniforms<'a>(
    drafts: impl IntoIterator<Item = &'a StageDraft>,
) -> Result<Vec<UboMember>, String> {
    let mut merged: Vec<UboMember> = vec![];
    let mut definitions = HashMap::new();
    for d in drafts {
        for m in &d.ubo {
            let dependencies = uniform_dependencies(m, &d.hoisted);
            for dependency in &dependencies {
                let spelling: Vec<_> = dependency.iter().map(Tok::text).collect();
                for name in declaration_names(dependency) {
                    let spelling = spelling.join(" ");
                    if definitions
                        .insert(name.clone(), spelling.clone())
                        .is_some_and(|old| old != spelling)
                    {
                        return Err(format!(
                            "uniform dependency `{name}` differs between stages"
                        ));
                    }
                }
            }
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
                None => {
                    let mut member = m.clone();
                    member.dependencies = dependencies;
                    merged.push(member);
                }
            }
        }
    }
    Ok(merged)
}

/// Names introduced by a hoisted struct or constant declaration.
fn declaration_names(tokens: &[Tok]) -> Vec<String> {
    let Some(declaration) = parse_decl(tokens) else {
        return Vec::new();
    };
    if let Some((name, _, _)) = declaration.block {
        vec![name]
    } else {
        declaration
            .decls
            .into_iter()
            .map(|(name, _, _)| name)
            .collect()
    }
}

/// Type names and constant references used by a declaration, excluding declared field names.
fn declaration_dependencies(tokens: &[Tok]) -> HashSet<String> {
    let mut needed = HashSet::new();
    let mut pending = vec![tokens.to_vec()];
    while let Some(tokens) = pending.pop() {
        let Some(declaration) = parse_decl(&tokens) else {
            continue;
        };
        if let Some((_, members, _)) = declaration.block {
            pending.extend(split_items(&members));
        } else {
            needed.insert(declaration.ty);
            needed.extend(
                declaration
                    .ty_arr
                    .iter()
                    .filter_map(Tok::ident)
                    .map(str::to_string),
            );
            for (_, array, initializer) in declaration.decls {
                needed.extend(
                    array
                        .iter()
                        .chain(&initializer)
                        .filter_map(Tok::ident)
                        .map(str::to_string),
                );
            }
        }
    }
    needed
}

/// Selects the transitive type and constant declarations needed by one uniform.
fn uniform_dependencies(member: &UboMember, hoisted: &[Tok]) -> Vec<Vec<Tok>> {
    let declarations = split_items(hoisted);
    let mut needed = declaration_dependencies(&member.toks);
    loop {
        let before = needed.len();
        for declaration in &declarations {
            if declaration_names(declaration)
                .iter()
                .any(|name| needed.contains(name))
            {
                needed.extend(declaration_dependencies(declaration));
            }
        }
        if needed.len() == before {
            break;
        }
    }
    declarations
        .into_iter()
        .filter(|declaration| {
            declaration_names(declaration)
                .iter()
                .any(|name| needed.contains(name))
        })
        .collect()
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
    let mut emitted = HashSet::new();
    for dependency in ubo.iter().flat_map(|member| &member.dependencies) {
        let names = declaration_names(dependency);
        if names.iter().any(|name| !emitted.contains(name)) {
            e.tokens(dependency);
            emitted.extend(names);
        }
    }
    for declaration in split_items(&draft.hoisted) {
        if declaration_names(&declaration)
            .iter()
            .all(|name| !emitted.contains(name))
        {
            e.tokens(&declaration);
        }
    }
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
    dependencies: Vec<Vec<Tok>>,
}

impl UboMember {
    /// Creates a named loose uniform from its complete declaration.
    pub(super) fn new(name: &str, toks: Vec<Tok>) -> UboMember {
        UboMember {
            name: name.to_string(),
            toks,
            dependencies: Vec::new(),
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
