//! Sign editor state: four lines of formatted text with a cursor, edited in place and sent back
//! as the whole sign compound.

use world::{NbtCompound, NbtValue};

pub const SIGN_LINES: usize = 4;
/// Widest a line may measure, in design pixels; longer input is refused.
pub const MAX_LINE_DESIGN_PIXELS: f32 = 90.0;

/// One sign face being edited.
#[derive(Clone, Debug, PartialEq)]
pub struct SignEdit {
    position: [i32; 3],
    front: bool,
    lines: [String; SIGN_LINES],
    original: [String; SIGN_LINES],
    line: usize,
    /// Cursor position in characters within the current line.
    column: usize,
    base: NbtCompound,
    color: [u8; 4],
    look: SignLook,
}

/// Which vanilla sign art and edit box the screen shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignLook {
    pub texture: String,
    pub hanging: bool,
}

impl SignLook {
    /// The `textures/ui` art for the sign block `name` (an oak sign when unknown).
    pub fn of_block(name: Option<&str>) -> Self {
        let name = name.unwrap_or_default();
        let name = name.strip_prefix("minecraft:").unwrap_or(name);
        let hanging = name.ends_with("hanging_sign");
        let wood = name
            .trim_end_matches("_hanging_sign")
            .trim_end_matches("hanging_sign")
            .trim_end_matches("_standing_sign")
            .trim_end_matches("standing_sign")
            .trim_end_matches("_wall_sign")
            .trim_end_matches("wall_sign")
            .trim_end_matches("_sign");
        let wood = match wood {
            "" | "oak" | "sign" => "",
            "dark_oak" | "darkoak" => "darkoak",
            other => other,
        };
        let texture = match (hanging, wood) {
            (true, "") => "hanging_sign".to_owned(),
            (true, wood) => format!("hanging_sign_{wood}"),
            (false, "") => "sign".to_owned(),
            // Newer woods name the art wood-first.
            (false, wood @ ("mangrove" | "bamboo" | "cherry" | "pale_oak")) => {
                format!("{wood}_sign")
            }
            (false, wood) => format!("sign_{wood}"),
        };
        Self {
            texture: format!("textures/ui/{texture}"),
            hanging,
        }
    }
}

fn face_key(front: bool) -> &'static str {
    if front { "FrontText" } else { "BackText" }
}

fn split_lines(text: &str) -> [String; SIGN_LINES] {
    let mut lines: [String; SIGN_LINES] = Default::default();
    for (slot, line) in lines.iter_mut().zip(text.split('\n')) {
        *slot = line.to_owned();
    }
    lines
}

impl SignEdit {
    /// Starts editing `front` of the sign whose current compound is `base`.
    pub fn new(position: [i32; 3], front: bool, base: NbtCompound) -> Self {
        let face = base.compound(face_key(front));
        // Signs from before the two-sided format keep the front text at the root.
        let source = face.or_else(|| front.then_some(&base));
        let text = source
            .and_then(|source| source.string("Text"))
            .unwrap_or_default();
        let argb = source
            .and_then(|source| source.integer("SignTextColor"))
            .and_then(|value| i32::try_from(value).ok())
            .unwrap_or(-0x0100_0000);
        let [_, red, green, blue] = argb.to_be_bytes();
        let lines = split_lines(text);
        Self {
            position,
            front,
            original: lines.clone(),
            lines,
            line: 0,
            column: 0,
            base,
            color: [red, green, blue, 255],
            look: SignLook::of_block(None),
        }
    }

    /// Shows the art of the sign block `name`.
    pub fn with_block(mut self, name: Option<&str>) -> Self {
        self.look = SignLook::of_block(name);
        self
    }

    pub const fn look(&self) -> &SignLook {
        &self.look
    }

    pub const fn position(&self) -> [i32; 3] {
        self.position
    }

    pub fn lines(&self) -> &[String; SIGN_LINES] {
        &self.lines
    }

    pub const fn cursor(&self) -> (usize, usize) {
        (self.line, self.column)
    }

    pub const fn color(&self) -> [u8; 4] {
        self.color
    }

    /// The edited text as the sign stores it, lines joined by newlines.
    /// The lines joined by newlines, without the empty trailing lines the editor pads with.
    pub fn text(&self) -> String {
        self.lines.join("\n").trim_end_matches('\n').to_owned()
    }

    pub fn changed(&self) -> bool {
        self.lines != self.original
    }

    fn byte_index(&self, column: usize) -> usize {
        self.lines[self.line]
            .char_indices()
            .nth(column)
            .map_or(self.lines[self.line].len(), |(index, _)| index)
    }

    fn line_chars(&self) -> usize {
        self.lines[self.line].chars().count()
    }

    /// Inserts `value` at the cursor when the widened line still satisfies `fits`.
    pub fn insert(&mut self, value: char, mut fits: impl FnMut(&str) -> bool) -> bool {
        if value.is_control() {
            return false;
        }
        let mut candidate = self.lines[self.line].clone();
        candidate.insert(self.byte_index(self.column), value);
        if !fits(&candidate) {
            return false;
        }
        self.lines[self.line] = candidate;
        self.column += 1;
        true
    }

    pub fn backspace(&mut self) {
        if self.column == 0 {
            return;
        }
        let end = self.byte_index(self.column);
        let start = self.byte_index(self.column - 1);
        self.lines[self.line].replace_range(start..end, "");
        self.column -= 1;
    }

    pub fn delete(&mut self) {
        if self.column >= self.line_chars() {
            return;
        }
        let start = self.byte_index(self.column);
        let end = self.byte_index(self.column + 1);
        self.lines[self.line].replace_range(start..end, "");
    }

    pub fn left(&mut self) {
        if self.column > 0 {
            self.column -= 1;
        } else if self.line > 0 {
            self.line -= 1;
            self.column = self.line_chars();
        }
    }

    pub fn right(&mut self) {
        if self.column < self.line_chars() {
            self.column += 1;
        } else if self.line + 1 < SIGN_LINES {
            self.line += 1;
            self.column = 0;
        }
    }

    pub fn up(&mut self) {
        if self.line > 0 {
            self.line -= 1;
            self.column = self.column.min(self.line_chars());
        }
    }

    pub fn down(&mut self) {
        if self.line + 1 < SIGN_LINES {
            self.line += 1;
            self.column = self.column.min(self.line_chars());
        }
    }

    pub fn home(&mut self) {
        self.column = 0;
    }

    pub fn end(&mut self) {
        self.column = self.line_chars();
    }

    /// Moves to the next line; `true` when already on the last, meaning the edit is finished.
    pub fn newline(&mut self) -> bool {
        if self.line + 1 >= SIGN_LINES {
            return true;
        }
        self.line += 1;
        self.column = 0;
        false
    }

    /// The whole sign compound with the edited face's text replaced, ready to send back. Both
    /// faces are always present, as servers require, and the block entity identity is kept.
    /// Fails if retained tags cannot be encoded within the NBT bounds.
    pub fn into_encoded_nbt(self) -> Result<Vec<u8>, &'static str> {
        let text = self.text();
        let mut root = self.base;
        // Hanging signs keep their own block-entity id.
        if root.string("id").is_none() {
            root.insert("id", NbtValue::String("Sign".into()));
        }
        for (axis, value) in ["x", "y", "z"].into_iter().zip(self.position) {
            root.insert(axis, NbtValue::Int(value));
        }
        for front in [true, false] {
            let mut face = root.compound(face_key(front)).cloned().unwrap_or_else(|| {
                let mut face = NbtCompound::default();
                if front {
                    for key in [
                        "Text",
                        "TextOwner",
                        "SignTextColor",
                        "IgnoreLighting",
                        "PersistFormatting",
                        "HideGlowOutline",
                        "FilteredText",
                        "FilteredTextOwner",
                    ] {
                        if let Some(value) = root.get(key) {
                            face.insert(key, value.clone());
                        }
                    }
                }
                face
            });
            if front == self.front {
                face.insert("Text", NbtValue::String(text.as_str().into()));
            } else if face.string("Text").is_none() {
                face.insert("Text", NbtValue::String("".into()));
            }
            root.insert(face_key(front), NbtValue::Compound(face));
        }
        root.encode_root()
    }
}

/// The open editor, if any.
#[derive(Clone, Debug, Default)]
pub struct SignEditor {
    active: Option<SignEdit>,
    finish_requested: bool, // closes and sends on the next input pass
}

impl SignEditor {
    pub const fn is_open(&self) -> bool {
        self.active.is_some()
    }

    pub fn open(&mut self, edit: SignEdit) {
        self.active = Some(edit);
        self.finish_requested = false;
    }

    /// Asks the input pass to finish the edit as Escape would.
    pub fn close_on_hurt(&mut self) {
        self.finish_requested = self.active.is_some();
    }

    pub fn take_finish_request(&mut self) -> bool {
        std::mem::take(&mut self.finish_requested)
    }

    pub fn active(&self) -> Option<&SignEdit> {
        self.active.as_ref()
    }

    pub fn active_mut(&mut self) -> Option<&mut SignEdit> {
        self.active.as_mut()
    }

    /// Keeps a completed edit open until the transport accepts its packet.
    pub fn finish(&mut self, send: impl FnOnce(protocol::Packet) -> Result<(), ()>) -> bool {
        let Some(edit) = self.active.clone() else {
            return true;
        };
        if !edit.changed() {
            self.close();
            return true;
        }
        let position = edit.position();
        let accepted = match edit.into_encoded_nbt() {
            Ok(nbt) => send(protocol::sign_edit_packet(position, &nbt)).is_ok(),
            Err(detail) => {
                bevy::log::warn!("sign edit NBT was not encodable: {detail}");
                false
            }
        };
        if accepted {
            self.close();
        } else {
            self.finish_requested = true;
        }
        accepted
    }

    pub fn close(&mut self) -> Option<SignEdit> {
        self.active.take()
    }
}

#[cfg(test)]
mod tests;
