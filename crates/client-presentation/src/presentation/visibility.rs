//! Visibility of the gameplay overlays controlled by Hide HUD and Hide Hand.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameplayOverlayVisibility {
    pub hand: bool,
    pub nametags: bool,
}

impl GameplayOverlayVisibility {
    /// Hide HUD suppresses both overlays while retaining the independent hand preference.
    pub const fn new(hide_hud: bool, hide_hand: bool) -> Self {
        Self {
            hand: !hide_hud && !hide_hand,
            nametags: !hide_hud,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hide_hud_retires_world_labels_and_hands_and_restores_the_hand_preference() {
        for hide_hand in [false, true] {
            let shown = GameplayOverlayVisibility::new(false, hide_hand);
            assert!(shown.nametags);
            assert_eq!(shown.hand, !hide_hand);
            assert_eq!(
                GameplayOverlayVisibility::new(true, hide_hand),
                GameplayOverlayVisibility {
                    hand: false,
                    nametags: false,
                }
            );
            assert_eq!(GameplayOverlayVisibility::new(false, hide_hand), shown);
        }
    }
}
