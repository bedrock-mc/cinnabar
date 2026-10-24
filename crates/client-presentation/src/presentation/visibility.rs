//! Visibility of the gameplay overlays controlled by Hide HUD and Hide Hand.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameplayOverlayVisibility {
    pub hand: bool,
    pub nametags: bool,
    /// The block outline or highlight drawn around the picked block.
    pub selection: bool,
}

impl GameplayOverlayVisibility {
    /// Hide HUD suppresses every overlay; spectators always hide their hands and held items.
    pub const fn new(hide_hud: bool, hide_hand: bool, spectator: bool) -> Self {
        Self {
            hand: !hide_hud && !hide_hand && !spectator,
            nametags: !hide_hud,
            selection: !hide_hud,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hide_hud_retires_world_labels_and_hands_and_restores_the_hand_preference() {
        for hide_hand in [false, true] {
            let shown = GameplayOverlayVisibility::new(false, hide_hand, false);
            assert!(shown.nametags);
            assert!(shown.selection);
            assert_eq!(shown.hand, !hide_hand);
            assert_eq!(
                GameplayOverlayVisibility::new(true, hide_hand, false),
                GameplayOverlayVisibility {
                    hand: false,
                    nametags: false,
                    selection: false,
                }
            );
            assert_eq!(
                GameplayOverlayVisibility::new(false, hide_hand, false),
                shown
            );
        }
    }

    #[test]
    fn gameplay_overlay_spectator_hides_hands_without_hiding_player_labels() {
        for (hide_hud, hide_hand) in [(false, false), (false, true), (true, false)] {
            let spectator = GameplayOverlayVisibility::new(hide_hud, hide_hand, true);
            assert!(!spectator.hand);
            assert_eq!(spectator.nametags, !hide_hud);
        }
        assert!(GameplayOverlayVisibility::new(false, false, false).hand);
    }
}
