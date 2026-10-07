use super::super::motion::{Surface, Tween};
use crate::menu::MenuAction;
use std::ops::Range;

const EDIT_SECONDS: f64 = 0.075;

#[derive(Clone, Debug)]
pub(in super::super) struct Insertion {
    pub(in super::super) characters: Range<usize>,
    pub(in super::super) progress: f32,
}

struct Field {
    surface: Surface,
    action: MenuAction,
    value: String,
    caret: Tween,
    target: f32,
    inserted: Range<usize>,
    reveal: Tween,
    touched: bool,
}

#[derive(Default)]
pub(in super::super) struct TextEdits(Vec<Field>);

impl TextEdits {
    pub(super) fn clear(&mut self) {
        self.0.clear();
    }

    pub(in super::super) fn sample(
        &mut self,
        surface: Surface,
        action: MenuAction,
        value: &str,
        caret: f32,
        seconds: f64,
        enabled: bool,
    ) -> (f32, Option<Insertion>) {
        if !enabled {
            return (caret, None);
        }
        let index = self
            .0
            .iter()
            .position(|f| f.surface == surface && f.action == action);
        let Some(index) = index else {
            self.0.push(Field {
                surface,
                action,
                value: value.to_owned(),
                caret: Tween::at(caret),
                target: caret,
                inserted: 0..0,
                reveal: Tween::at(1.0),
                touched: true,
            });
            return (caret, None);
        };
        let field = &mut self.0[index];
        field.touched = true;
        if field.value != value {
            field.inserted = inserted_characters(&field.value, value);
            field.value.clear();
            field.value.push_str(value);
            field.caret.retarget(caret, EDIT_SECONDS, seconds);
            field.reveal = Tween::at(0.0);
            field.reveal.retarget(1.0, EDIT_SECONDS, seconds);
        } else if field.target != caret {
            // Pointer and navigation moves place the caret immediately.
            field.caret = Tween::at(caret);
        }
        field.target = caret;
        let progress = field.reveal.sample(seconds);
        let insertion = (progress < 1.0 && !field.inserted.is_empty()).then(|| Insertion {
            characters: field.inserted.clone(),
            progress,
        });
        (field.caret.sample(seconds), insertion)
    }

    pub(super) fn end_frame(&mut self) {
        self.0
            .retain_mut(|field| std::mem::take(&mut field.touched));
    }
}

fn inserted_characters(before: &str, after: &str) -> Range<usize> {
    let prefix = before
        .chars()
        .zip(after.chars())
        .take_while(|(a, b)| a == b)
        .count();
    let limit = before.chars().count().min(after.chars().count()) - prefix;
    let suffix = before
        .chars()
        .rev()
        .zip(after.chars().rev())
        .take(limit)
        .take_while(|(a, b)| a == b)
        .count();
    prefix..after.chars().count() - suffix
}

#[cfg(test)]
mod tests {
    use super::*;
    const SURFACE: Surface = Surface::Loading;
    const ACTION: MenuAction = MenuAction::AddName;

    #[test]
    fn mounting_and_unchanged_values_are_settled() {
        let mut edits = TextEdits::default();
        assert!(
            edits
                .sample(SURFACE, ACTION, "Existing", 12.0, 0.0, true)
                .1
                .is_none()
        );
        assert!(
            edits
                .sample(SURFACE, ACTION, "Existing", 12.0, 0.01, true)
                .1
                .is_none()
        );
        assert_eq!(
            edits.sample(SURFACE, ACTION, "Existing", 4.0, 0.02, true).0,
            4.0
        );
    }

    #[test]
    fn typing_reveals_only_the_inserted_unicode_characters() {
        assert_eq!(inserted_characters("A猫C", "A猫🐧BC"), 2..4);
        assert_eq!(inserted_characters("A猫C", "AC"), 1..1);
        let mut edits = TextEdits::default();
        edits.sample(SURFACE, ACTION, "A", 10.0, 0.0, true);
        let (caret, inserted) = edits.sample(SURFACE, ACTION, "AB", 20.0, 0.1, true);
        assert_eq!(caret, 10.0);
        assert_eq!(inserted.unwrap().characters, 1..2);
        let (caret, inserted) = edits.sample(SURFACE, ACTION, "AB", 20.0, 0.125, true);
        assert!(caret > 10.0 && caret < 20.0);
        assert!(inserted.unwrap().progress > 0.0);
        assert_eq!(edits.sample(SURFACE, ACTION, "AB", 20.0, 0.2, true).0, 20.0);
        assert!(
            edits
                .sample(SURFACE, ACTION, "AB", 20.0, 0.2, true)
                .1
                .is_none()
        );
    }

    #[test]
    fn rapid_edits_preserve_caret_continuity_and_disabled_motion_is_immediate() {
        let mut edits = TextEdits::default();
        edits.sample(SURFACE, ACTION, "A", 10.0, 0.0, true);
        edits.sample(SURFACE, ACTION, "AB", 20.0, 0.1, true);
        let before = edits.sample(SURFACE, ACTION, "AB", 20.0, 0.12, true).0;
        assert_eq!(
            edits.sample(SURFACE, ACTION, "ABC", 30.0, 0.12, true).0,
            before
        );
        assert_eq!(
            edits.sample(SURFACE, ACTION, "ABCD", 40.0, 0.13, false).0,
            40.0
        );
        assert!(
            edits
                .sample(SURFACE, ACTION, "ABCD", 40.0, 0.13, false)
                .1
                .is_none()
        );
    }

    #[test]
    fn unmounted_fields_release_their_text_and_motion() {
        let mut edits = TextEdits::default();
        edits.sample(SURFACE, ACTION, "Old", 10.0, 0.0, true);
        edits.end_frame();
        edits.end_frame();
        assert!(edits.0.is_empty());
        assert!(
            edits
                .sample(SURFACE, ACTION, "New", 20.0, 0.1, true)
                .1
                .is_none()
        );
    }
}
