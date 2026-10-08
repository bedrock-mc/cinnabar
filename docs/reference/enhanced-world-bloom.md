# Enhanced Bloom composition

Enhanced is a Cinnabar extension. Its world Bloom and grade finish before the
first-person hand, held items, HUD and menus draw. World-projected text remains
part of the world pass. Cameras without Enhanced retain the main-pass route.

The old graph graded before Bloom and drew the hand and UI before Bloom. Moving
Bloom before `EndMainPass` would make a cycle through Bevy's motion-blur chain.
The conditional post-grade twins leave that chain intact and draw each view once.
Each installed twin is also ordered before tonemapping when the UI pass is absent.

## Vanilla rules

| Rule | Behaviour |
| --- | --- |
| World post-effects | Bloom uses a fullscreen blend pass; this does not prescribe Cinnabar’s graph labels. |
| First-person items | First-person items have a dedicated screen render path. |
| HUD and settings | Installed `v1.26.50.4/full/resource_pack/ui/hud_screen.json` and `ui/settings_screen.json` are separate JSON-UI screens; Enhanced world effects do not change these authored controls. |

The graph regression checks Bloom → grade → hand/UI ordering, preserves the
motion-blur dependency, and checks that the new edges do not return to Bloom or
`EndMainPass`. Native offscreen verification is local evidence; this does not
close a vanilla graphics parity gate or authorize a push.
