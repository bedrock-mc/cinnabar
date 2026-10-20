//! The layer-to-control mapping: containers to panels and stack panels, text
//! to labels, fills and `@image` exports to images.

use std::collections::BTreeMap;
use std::io::Cursor;

use serde_json::{Map, Value, json};

use super::names::{self, Directives, NameSet};
use super::place::{Place, anchor, number, place_control, stretch};
use super::{Align, AutoLayout, Conversion, LayoutMode, Node, Paint, Positioning, Warning};
use crate::mock::MockData;

mod button;

use button::button;

pub(super) type Body = Map<String, Value>;

/// The Figma font size drawn at `font_scale_factor` 1: the default font's 8 px glyph cell.
const BASE_FONT_SIZE: f64 = 8.0;
/// Image key of the generated white texture that solid fills tint.
const SOLID_KEY: &str = "\0solid";
const CONTAINERS: [&str; 6] = [
    "FRAME",
    "GROUP",
    "COMPONENT",
    "COMPONENT_SET",
    "INSTANCE",
    "SECTION",
];

/// Converts `root` into a pack. `images` holds the bytes behind every image
/// key the tree names (fills' image hashes and `@image` exports).
pub fn convert(root: &Node, images: &BTreeMap<String, Vec<u8>>) -> Conversion {
    let namespace = names::identifier(&names::parse(&root.name).words, "figma_screen");
    let mut ctx = Ctx {
        namespace: namespace.clone(),
        images,
        files: BTreeMap::new(),
        textures: BTreeMap::new(),
        stems: NameSet::default(),
        definitions: BTreeMap::new(),
        top: NameSet::default(),
        warnings: Vec::new(),
        mock: MockData::default(),
    };
    let root_name = ctx.top.unique(&namespace);
    let screen_name = ctx.top.unique(&format!("{root_name}_screen"));
    let body = ctx
        .control(root, Place::Fill, &Scope::default())
        .map_or_else(|| panel(Vec::new()), |(_, body)| body);
    ctx.definitions
        .insert(root_name.clone(), Value::Object(body));
    ctx.definitions.insert(
        screen_name.clone(),
        json!({
            "type": "screen",
            "controls": [{ format!("{root_name}@{namespace}.{root_name}"): {} }],
        }),
    );
    let ui_path = format!("ui/{namespace}.json");
    ctx.files
        .insert(ui_path.clone(), ui_file(&namespace, &ctx.definitions));
    ctx.files.insert(
        json_ui::UI_DEFS_PATH.to_owned(),
        pretty(&json!({ "ui_defs": [ui_path] })),
    );
    Conversion {
        screen: format!("{namespace}.{screen_name}"),
        namespace,
        size: [root.width, root.height],
        files: ctx.files,
        warnings: ctx.warnings,
        mock: ctx.mock,
    }
}

#[derive(Clone, Default)]
struct Scope {
    /// The parent's layer path.
    path: String,
    /// The factory collection and item index that bound values sample into.
    item: Option<(String, usize)>,
}

impl Scope {
    fn path_to(&self, node: &Node) -> String {
        if self.path.is_empty() {
            node.name.clone()
        } else {
            format!("{} / {}", self.path, node.name)
        }
    }
}

struct Ctx<'a> {
    namespace: String,
    images: &'a BTreeMap<String, Vec<u8>>,
    files: BTreeMap<String, Vec<u8>>,
    /// Image key to exported texture path.
    textures: BTreeMap<String, String>,
    stems: NameSet,
    /// Top-level controls of the UI file.
    definitions: BTreeMap<String, Value>,
    top: NameSet,
    warnings: Vec<Warning>,
    mock: MockData,
}

impl Ctx<'_> {
    fn warn(&mut self, node: &Node, path: &str, message: impl Into<String>) {
        self.warnings.push(Warning {
            node_id: node.id.clone(),
            layer: path.to_owned(),
            message: message.into(),
        });
    }

    /// `node` as a named control placed by `place`; `None` when it has no
    /// JSON-UI form (after warning why).
    fn control(&mut self, node: &Node, place: Place, scope: &Scope) -> Option<(String, Body)> {
        let directives = names::parse(&node.name);
        let path = scope.path_to(node);
        for token in &directives.unknown {
            self.warn(node, &path, format!("`{token}` is not a known directive"));
        }
        let inner = Scope {
            path: path.clone(),
            item: scope.item.clone(),
        };
        let variants = match (&directives.button, &node.raster) {
            (Some(_), None) => self.state_variants(node, &inner),
            _ => None,
        };
        let (mut body, auto, extent) = match variants {
            Some((states, extent)) => (states, [false; 2], extent),
            None => {
                self.check_common(node, &path);
                let (body, auto) = self.content(node, &path, &directives, scope, &inner)?;
                (body, auto, [node.width, node.height])
            }
        };
        if directives.image && node.raster.is_none() {
            let message = format!(
                "`{}` arrived without its export; converted as layers",
                names::IMAGE
            );
            self.warn(node, &path, message);
        }
        let name = names::identifier(&directives.words, &node.kind.to_ascii_lowercase());
        if let Some(id) = &directives.button {
            body = button(body, id.as_deref().unwrap_or(&name));
        }
        if !node.visible {
            body.insert("visible".into(), false.into());
        }
        if let Some(binding) = &directives.visible {
            self.bind(&mut body, "visible", binding, node.visible.into(), scope);
        }
        if node.opacity < 0.999 {
            if matches!(type_of(&body), "label" | "image") {
                let alpha = body.get("alpha").and_then(Value::as_f64).unwrap_or(1.0);
                set_alpha(&mut body, alpha * node.opacity);
            } else {
                set_alpha(&mut body, node.opacity);
                body.insert("propagate_alpha".into(), true.into());
            }
        }
        place_control(&mut body, node, extent, auto, place);
        Some((name, body))
    }

    /// `node`'s own control before placement, and which axes size to content.
    fn content(
        &mut self,
        node: &Node,
        path: &str,
        directives: &Directives,
        scope: &Scope,
        inner: &Scope,
    ) -> Option<(Body, [bool; 2])> {
        Some(match (node.raster.as_deref(), node.kind.as_str()) {
            (Some(key), _) => (self.raster(node, key, path, directives, scope)?, [false; 2]),
            (None, "TEXT") => self.text(node, path, directives, scope)?,
            (None, "RECTANGLE") => (self.rectangle(node, path, directives, scope), [false; 2]),
            (None, kind) if CONTAINERS.contains(&kind) => {
                (self.container(node, path, directives, inner), [false; 2])
            }
            (None, kind) => {
                let message = format!(
                    "{} layers are not supported; name it `{}` to export it as a texture",
                    label(kind),
                    names::IMAGE
                );
                self.warn(node, path, message);
                return None;
            }
        })
    }

    /// Warnings for properties no control expresses.
    fn check_common(&mut self, node: &Node, path: &str) {
        if node.rotation.abs() > 0.01 {
            self.warn(node, path, "rotation is not supported");
        }
        if !matches!(node.blend_mode.as_str(), "" | "NORMAL" | "PASS_THROUGH") {
            let mode = label(&node.blend_mode);
            self.warn(node, path, format!("{mode} blend mode is not supported"));
        }
        if node.is_mask {
            self.warn(node, path, "masks are not supported");
        }
        // An `@image` export already contains its effects, strokes and corners;
        // a component set's own fill and stroke are canvas chrome.
        if node.raster.is_some() || node.kind == "COMPONENT_SET" {
            return;
        }
        for effect in node.effects.iter().filter(|effect| effect.visible) {
            let kind = label(&effect.kind);
            self.warn(node, path, format!("{kind} effect is not supported"));
        }
        if node.strokes.iter().any(|stroke| stroke.visible) {
            self.warn(node, path, "strokes are not supported");
        }
        if node.corner_radius > 0.0 && (node.clips_content || visible_fills(node).next().is_some())
        {
            self.warn(node, path, "corner radius is not supported");
        }
    }

    fn raster(
        &mut self,
        node: &Node,
        key: &str,
        path: &str,
        directives: &Directives,
        scope: &Scope,
    ) -> Option<Body> {
        let texture = self.texture(node, key, path)?;
        let mut body = image(&texture);
        body.insert("keep_ratio".into(), false.into());
        if let Some(binding) = &directives.binding {
            self.bind(&mut body, "texture", binding, texture.into(), scope);
        }
        Some(body)
    }

    fn text(
        &mut self,
        node: &Node,
        path: &str,
        directives: &Directives,
        scope: &Scope,
    ) -> Option<(Body, [bool; 2])> {
        let Some(text) = &node.text else {
            self.warn(node, path, "text layer arrived without its text");
            return None;
        };
        let mut body = Body::new();
        body.insert("type".into(), "label".into());
        body.insert("text".into(), text.characters.clone().into());
        for property in &text.mixed {
            let message = format!(
                "mixed {property} within one text is not supported; the first run's is used"
            );
            self.warn(node, path, message);
        }
        let fills: Vec<&Paint> = visible_fills(node).collect();
        match fills.last() {
            None => {
                self.warn(
                    node,
                    path,
                    "text without a visible fill is exported transparent",
                );
                body.insert("alpha".into(), 0.into());
            }
            Some(paint) => {
                if fills.len() > 1 {
                    self.warn(
                        node,
                        path,
                        "stacked text fills are not supported; the top fill is used",
                    );
                }
                match (paint.kind.as_str(), paint.color) {
                    ("SOLID", Some(rgb)) => {
                        body.insert("color".into(), color(rgb));
                        set_alpha(&mut body, paint.opacity);
                    }
                    (kind, _) => {
                        let message = format!(
                            "{} text fills are not supported; the text is white",
                            label(kind)
                        );
                        self.warn(node, path, message);
                    }
                }
            }
        }
        let (font, known) = font_type(&text.font_family);
        if font != "default" {
            body.insert("font_type".into(), font.into());
        }
        if !known {
            let message = format!(
                "font `{}` has no Bedrock equivalent; it uses the smooth font",
                text.font_family
            );
            self.warn(node, path, message);
        }
        if !matches!(
            text.font_style.to_ascii_lowercase().as_str(),
            "" | "regular"
        ) {
            let message = format!("font style `{}` is not supported", text.font_style);
            self.warn(node, path, message);
        }
        let scale = text.font_size / BASE_FONT_SIZE;
        if (scale - 1.0).abs() > 1e-3 {
            body.insert("font_scale_factor".into(), fraction(scale));
        }
        match text.align_h.as_str() {
            "CENTER" => {
                body.insert("text_alignment".into(), "center".into());
            }
            "RIGHT" => {
                body.insert("text_alignment".into(), "right".into());
            }
            "JUSTIFIED" => self.warn(
                node,
                path,
                "justified text is not supported; it aligns left",
            ),
            _ => {}
        }
        let unsupported = [
            (text.letter_spacing != 0.0, "letter spacing"),
            (text.line_height.is_some(), "a fixed line height"),
            (
                !matches!(text.text_case.as_str(), "" | "ORIGINAL"),
                "text case",
            ),
            (
                !matches!(text.text_decoration.as_str(), "" | "NONE"),
                "text decoration",
            ),
            (
                text.truncate || text.auto_resize == "TRUNCATE",
                "text truncation",
            ),
        ];
        for (_, what) in unsupported.into_iter().filter(|(set, _)| *set) {
            self.warn(node, path, format!("{what} is not supported"));
        }
        if let Some(binding) = &directives.binding {
            self.bind(
                &mut body,
                "text",
                binding,
                text.characters.clone().into(),
                scope,
            );
        }
        let auto = match text.auto_resize.as_str() {
            "WIDTH_AND_HEIGHT" => [true, true],
            "HEIGHT" => [false, true],
            _ => [false, false],
        };
        let vertical = match text.align_v.as_str() {
            "CENTER" => Some("left_middle"),
            "BOTTOM" => Some("bottom_left"),
            _ => None,
        };
        // Labels have no vertical alignment: a fixed-height box anchors its text instead.
        match vertical {
            Some(anchor) if !auto[1] => {
                body.insert("size".into(), json!(["100%", "default"]));
                body.insert("anchor_from".into(), anchor.into());
                body.insert("anchor_to".into(), anchor.into());
                Some((panel(vec![entry("text", body)]), auto))
            }
            _ => Some((body, auto)),
        }
    }

    fn rectangle(
        &mut self,
        node: &Node,
        path: &str,
        directives: &Directives,
        scope: &Scope,
    ) -> Body {
        let mut fills = self.fills(node, path);
        if fills.len() == 1 {
            let (kind, mut body) = fills.remove(0);
            if let Some(binding) = &directives.binding {
                match body.get("texture").and_then(Value::as_str) {
                    Some(texture) if kind == "IMAGE" => {
                        let texture = texture.to_owned();
                        self.bind(&mut body, "texture", binding, texture.into(), scope);
                    }
                    _ => self.warn(node, path, "`$binding` binds a text or image layer"),
                }
            }
            return body;
        }
        if directives.binding.is_some() {
            self.warn(node, path, "`$binding` binds a text or image layer");
        }
        let mut names = NameSet::default();
        let controls = fills
            .into_iter()
            .map(|(_, mut fill)| {
                fill.insert("size".into(), json!(["100%", "100%"]));
                entry(&names.unique("fill"), fill)
            })
            .collect();
        panel(controls)
    }

    fn container(
        &mut self,
        node: &Node,
        path: &str,
        directives: &Directives,
        scope: &Scope,
    ) -> Body {
        if directives.binding.is_some() {
            self.warn(node, path, "`$binding` binds a text or image layer");
        }
        let mut names = NameSet::default();
        let fills = match node.kind.as_str() {
            "COMPONENT_SET" => Vec::new(),
            _ => self.fills(node, path),
        };
        let mut controls: Vec<Value> = fills
            .into_iter()
            .map(|(_, mut fill)| {
                fill.insert("size".into(), json!(["100%", "100%"]));
                entry(&names.unique("background"), fill)
            })
            .collect();
        let mut body = match &node.layout {
            Some(layout) if layout.mode != LayoutMode::Other => {
                let factory = directives.factory.as_deref();
                self.auto_layout(node, layout, path, factory, scope, (names, controls))
            }
            layout => {
                if layout.is_some() {
                    self.warn(
                        node,
                        path,
                        "this auto-layout mode is not supported; children keep their positions",
                    );
                }
                if directives.factory.is_some() {
                    self.warn(node, path, "`@factory` needs an auto-layout frame");
                }
                let parent = [node.width, node.height];
                for child in &node.children {
                    if let Some((name, body)) = self.control(child, Place::Free { parent }, scope) {
                        controls.push(entry(&names.unique(&name), body));
                    }
                }
                panel(controls)
            }
        };
        if node.clips_content {
            body.insert("clips_children".into(), true.into());
        }
        body
    }

    /// A stack panel, wrapped in a panel when backgrounds, padding, primary-axis
    /// alignment or absolutely positioned children need one.
    fn auto_layout(
        &mut self,
        node: &Node,
        layout: &AutoLayout,
        path: &str,
        factory: Option<&str>,
        scope: &Scope,
        (mut names, mut outer): (NameSet, Vec<Value>),
    ) -> Body {
        let main = usize::from(layout.mode != LayoutMode::Horizontal);
        let cross = 1 - main;
        if layout.wrap {
            self.warn(node, path, "wrapping auto-layout is not supported");
        }
        let primary = match layout.primary_align {
            align @ (Align::Min | Align::Center | Align::Max) => align,
            align => {
                let message = format!(
                    "{align:?} distribution is not supported; children pack from the start"
                );
                self.warn(node, path, message);
                Align::Min
            }
        };
        let counter = match layout.counter_align {
            align @ (Align::Min | Align::Center | Align::Max) => align,
            align => {
                self.warn(
                    node,
                    path,
                    format!("{align:?} alignment is not supported; children align to the start"),
                );
                Align::Min
            }
        };
        let anchored = counter != Align::Min;
        let place = Place::Stack {
            main,
            counter,
            anchored,
        };
        let mut stack = Body::new();
        stack.insert("type".into(), "stack_panel".into());
        let orientation = if main == 0 { "horizontal" } else { "vertical" };
        stack.insert("orientation".into(), orientation.into());
        if anchored {
            stack.insert("use_child_anchors".into(), true.into());
        }
        let (flow, absolute): (Vec<&Node>, Vec<&Node>) = node
            .children
            .iter()
            .partition(|child| child.positioning == Positioning::Auto);
        if layout.item_spacing < 0.0 {
            self.warn(node, path, "negative spacing is not supported");
        }
        if let Some(collection) = factory {
            if layout.item_spacing > 0.0 {
                self.warn(node, path, "spacing between factory items is not supported");
            }
            self.factory(node, &flow, collection, place, &mut stack, path);
        } else {
            let mut inner = NameSet::default();
            let mut controls = Vec::new();
            let mut shown = false;
            for child in flow {
                if child.visible && shown && layout.item_spacing > 0.0 {
                    let mut size = [json!(0), json!(0)];
                    size[main] = number(layout.item_spacing);
                    let spacer = json!({ "type": "panel", "size": size });
                    controls.push(json!({ inner.unique("spacing"): spacer }));
                }
                if let Some((name, body)) = self.control(child, place, scope) {
                    controls.push(entry(&inner.unique(&name), body));
                    shown |= child.visible;
                }
            }
            if !controls.is_empty() {
                stack.insert("controls".into(), controls.into());
            }
        }
        let [left, top, right, bottom] = layout.padding;
        let (start, end) = ([left, top], [right, bottom]);
        let padded = layout.padding.iter().any(|pad| *pad != 0.0);
        if outer.is_empty() && !padded && primary == Align::Min && absolute.is_empty() {
            return stack;
        }
        let mut size = [Value::Null, Value::Null];
        let mut offset = [Value::Null, Value::Null];
        let mut parts = [0; 2];
        size[cross] = stretch(start[cross] + end[cross]);
        offset[cross] = number(start[cross]);
        (parts[main], offset[main], size[main]) = match primary {
            Align::Center => (1, number((start[main] - end[main]) / 2.0), json!("100%c")),
            Align::Max => (2, number(-end[main]), json!("100%c")),
            _ => (0, number(start[main]), stretch(start[main] + end[main])),
        };
        stack.insert("size".into(), json!(size));
        stack.insert("offset".into(), json!(offset));
        anchor(&mut stack, parts);
        outer.push(entry(&names.unique("content"), stack));
        let parent = [node.width, node.height];
        for child in absolute {
            if let Some((name, body)) = self.control(child, Place::Free { parent }, scope) {
                outer.push(entry(&names.unique(&name), body));
            }
        }
        panel(outer)
    }

    /// Makes `stack` a collection factory of its first child; every child
    /// supplies one preview item's bound values.
    fn factory(
        &mut self,
        node: &Node,
        flow: &[&Node],
        collection: &str,
        place: Place,
        stack: &mut Body,
        path: &str,
    ) {
        let Some((template, samples)) = flow.split_first() else {
            self.warn(node, path, "`@factory` needs a child to repeat");
            return;
        };
        let scope = Scope {
            path: path.to_owned(),
            item: Some((collection.to_owned(), 0)),
        };
        let Some((name, body)) = self.control(template, place, &scope) else {
            return;
        };
        let name = self.top.unique(&name);
        self.definitions.insert(name.clone(), Value::Object(body));
        for (index, sample) in samples.iter().enumerate() {
            self.samples(sample, &scope.path_to(sample), collection, index + 1);
        }
        for index in 0..flow.len() {
            self.item(collection, index)
                .insert("role".into(), name.clone().into());
        }
        let reference = format!("@{}.{name}", self.namespace);
        stack.insert("collection_name".into(), collection.into());
        stack.insert(
            "factory".into(),
            json!({ "name": format!("{collection}_factory"), "control_ids": { name: reference } }),
        );
    }

    /// Records a factory sample's bound values without converting it.
    fn samples(&mut self, node: &Node, path: &str, collection: &str, index: usize) {
        let directives = names::parse(&node.name);
        if let Some(binding) = &directives.binding {
            let image = node.raster.as_deref().or_else(|| {
                let fills: Vec<&Paint> = visible_fills(node).collect();
                match fills.as_slice() {
                    [paint] if paint.kind == "IMAGE" => paint.image.as_deref(),
                    _ => None,
                }
            });
            let value = match (&node.text, image) {
                (Some(text), _) => Some(text.characters.clone().into()),
                (None, Some(key)) => self.texture(node, key, path).map(Value::from),
                (None, None) => None,
            };
            if let Some(value) = value {
                self.item(collection, index).insert(binding.clone(), value);
            }
        }
        if let Some(binding) = &directives.visible {
            self.item(collection, index)
                .insert(binding.clone(), node.visible.into());
        }
        if node.raster.is_none() {
            for child in &node.children {
                self.samples(
                    child,
                    &format!("{path} / {}", child.name),
                    collection,
                    index,
                );
            }
        }
    }

    fn item(&mut self, collection: &str, index: usize) -> &mut BTreeMap<String, Value> {
        let items = self
            .mock
            .collections
            .entry(collection.to_owned())
            .or_default();
        if items.len() <= index {
            items.resize_with(index + 1, BTreeMap::new);
        }
        &mut items[index]
    }

    /// Binds `property` (`text`, `texture` or `visible`) to `binding`, sampling
    /// `value` for previews.
    fn bind(
        &mut self,
        body: &mut Body,
        property: &str,
        binding: &str,
        value: Value,
        scope: &Scope,
    ) {
        let mut entry = Body::new();
        entry.insert("binding_name".into(), binding.into());
        if property == "text" {
            body.insert("text".into(), binding.into());
        } else {
            entry.insert(
                "binding_name_override".into(),
                format!("#{property}").into(),
            );
        }
        match &scope.item {
            Some((collection, index)) => {
                entry.insert("binding_type".into(), "collection".into());
                entry.insert("binding_collection_name".into(), collection.clone().into());
                self.item(collection, *index)
                    .insert(binding.to_owned(), value);
            }
            None => {
                self.mock.globals.insert(binding.to_owned(), value);
            }
        }
        if let Value::Array(bindings) = body.entry("bindings").or_insert_with(|| json!([])) {
            bindings.push(Value::Object(entry));
        }
    }

    /// Image controls for `node`'s visible fills, bottom first, with their Figma paint type.
    fn fills(&mut self, node: &Node, path: &str) -> Vec<(String, Body)> {
        let mut out = Vec::new();
        for paint in visible_fills(node) {
            let mut body = match (paint.kind.as_str(), paint.color, paint.image.as_deref()) {
                ("SOLID", Some(rgb), _) => {
                    let mut body = image(&self.solid());
                    body.insert("color".into(), color(rgb));
                    body.insert("keep_ratio".into(), false.into());
                    body
                }
                ("IMAGE", _, Some(key)) => {
                    let Some(texture) = self.texture(node, key, path) else {
                        continue;
                    };
                    let mut body = image(&texture);
                    match paint.scale_mode.as_str() {
                        "FILL" | "" => {
                            body.insert("fill".into(), true.into());
                        }
                        "FIT" => {
                            body.insert("keep_ratio".into(), true.into());
                        }
                        mode => {
                            let message = format!(
                                "{} image scaling is not supported; the image stretches",
                                label(mode)
                            );
                            self.warn(node, path, message);
                            body.insert("keep_ratio".into(), false.into());
                        }
                    }
                    if paint.has_filters {
                        self.warn(node, path, "image adjustments are not supported");
                    }
                    body
                }
                (kind, _, _) => {
                    self.warn(
                        node,
                        path,
                        format!("{} fills are not supported", label(kind)),
                    );
                    continue;
                }
            };
            set_alpha(&mut body, paint.opacity);
            out.push((paint.kind.clone(), body));
        }
        out
    }

    /// The pack texture for image `key`, exported as PNG on first use.
    fn texture(&mut self, node: &Node, key: &str, path: &str) -> Option<String> {
        if let Some(texture) = self.textures.get(key) {
            return Some(texture.clone());
        }
        let Some(bytes) = self.images.get(key) else {
            self.warn(node, path, "the image's data is missing");
            return None;
        };
        let png = match to_png(bytes) {
            Ok(png) => png,
            Err(error) => {
                self.warn(node, path, format!("the image could not be read: {error}"));
                return None;
            }
        };
        let stem = names::identifier(&names::parse(&node.name).words, "image");
        Some(self.add_texture(key, &stem, png))
    }

    /// The white texture solid fills tint.
    fn solid(&mut self) -> String {
        if let Some(texture) = self.textures.get(SOLID_KEY) {
            return texture.clone();
        }
        let white = image::RgbaImage::from_pixel(1, 1, image::Rgba([255; 4]));
        let mut png = Cursor::new(Vec::new());
        white
            .write_to(&mut png, image::ImageFormat::Png)
            .expect("encoding a 1x1 PNG in memory cannot fail");
        self.add_texture(SOLID_KEY, "solid", png.into_inner())
    }

    fn add_texture(&mut self, key: &str, stem: &str, png: Vec<u8>) -> String {
        let texture = format!("textures/{}/{}", self.namespace, self.stems.unique(stem));
        self.files.insert(format!("{texture}.png"), png);
        self.textures.insert(key.to_owned(), texture.clone());
        texture
    }
}

/// PNG bytes as given; other formats decoded and re-encoded as PNG.
fn to_png(bytes: &[u8]) -> Result<Vec<u8>, String> {
    match image::guess_format(bytes).map_err(|error| error.to_string())? {
        image::ImageFormat::Png => Ok(bytes.to_vec()),
        _ => {
            let decoded = image::load_from_memory(bytes).map_err(|error| error.to_string())?;
            let mut png = Cursor::new(Vec::new());
            decoded
                .write_to(&mut png, image::ImageFormat::Png)
                .map_err(|error| error.to_string())?;
            Ok(png.into_inner())
        }
    }
}

fn visible_fills(node: &Node) -> impl Iterator<Item = &Paint> {
    node.fills
        .iter()
        .filter(|paint| paint.visible && paint.opacity > 0.0)
}

/// The Bedrock `font_type` for a Figma font family, and whether it is a match.
fn font_type(family: &str) -> (&'static str, bool) {
    let family = family.to_ascii_lowercase().replace([' ', '-', '_'], "");
    if family.contains("minecraftten") {
        ("MinecraftTen", true)
    } else if family.contains("minecraft") || family.contains("mojangles") {
        ("default", true)
    } else {
        ("smooth", false)
    }
}

fn type_of(body: &Body) -> &str {
    body.get("type").and_then(Value::as_str).unwrap_or_default()
}

fn image(texture: &str) -> Body {
    let mut body = Body::new();
    body.insert("type".into(), "image".into());
    body.insert("texture".into(), texture.into());
    body
}

fn panel(controls: Vec<Value>) -> Body {
    let mut body = Body::new();
    body.insert("type".into(), "panel".into());
    if !controls.is_empty() {
        body.insert("controls".into(), controls.into());
    }
    body
}

fn entry(name: &str, body: Body) -> Value {
    json!({ name: Value::Object(body) })
}

fn set_alpha(body: &mut Body, alpha: f64) {
    if alpha < 0.999 {
        body.insert("alpha".into(), fraction(alpha.max(0.0)));
    } else {
        body.remove("alpha");
    }
}

/// `SOME_ENUM` as `some enum` for messages.
fn label(kind: &str) -> String {
    kind.to_ascii_lowercase().replace('_', " ")
}

fn color(rgb: [f64; 3]) -> Value {
    json!(rgb.map(|channel| fraction(channel.clamp(0.0, 1.0))))
}

/// Ratios at 1/1000 precision.
fn fraction(value: f64) -> Value {
    let rounded = (value * 1000.0).round() / 1000.0;
    if rounded.fract() == 0.0 {
        json!(rounded as i64)
    } else {
        json!(rounded)
    }
}

fn pretty(value: &Value) -> Vec<u8> {
    let mut text = serde_json::to_string_pretty(value).unwrap_or_default();
    text.push('\n');
    text.into_bytes()
}

/// The UI file with `namespace` first, as authors write it.
fn ui_file(namespace: &str, definitions: &BTreeMap<String, Value>) -> Vec<u8> {
    let body = serde_json::to_string_pretty(definitions).unwrap_or_default();
    format!("{{\n  \"namespace\": {},{}\n", json!(namespace), &body[1..]).into_bytes()
}
