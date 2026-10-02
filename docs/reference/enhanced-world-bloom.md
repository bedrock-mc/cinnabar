# Enhanced Bloom composition

Enhanced is a Cinnabar extension. Its world Bloom and grade finish before the
first-person hand, held items, HUD and menus draw. World-projected text remains
part of the world pass. Cameras without Enhanced retain the main-pass route.

The old graph graded before Bloom and drew the hand and UI before Bloom. Moving
Bloom before `EndMainPass` would make a cycle through Bevy's motion-blur chain.
The conditional post-grade twins leave that chain intact and draw each view once.
Each installed twin is also ordered before tonemapping when the UI pass is absent.

References inspected:

- Lens, reconstructed client `1.26.50.26`, artifact 6, RVA `0x0cbf1100`,
  source-backed canonical `BloomBlend` fullscreen pass (line 250). This identifies
  the world post-effect mechanism; it does not specify Cinnabar's graph labels.
- `R:i/InGamePlayScreen.cpp:7011` and `R:i/InGamePlayScreen.cpp:7559`: the
  first-person item has a dedicated screen render path.
- Installed `v1.26.50.4/full/resource_pack/ui/hud_screen.json` and
  `ui/settings_screen.json`: HUD and settings are separate JSON-UI screens.
  Enhanced's world effects do not change these authored screen controls.

The graph regression checks Bloom → grade → hand/UI ordering, preserves the
motion-blur dependency, and checks that the new edges do not return to Bloom or
`EndMainPass`. Native offscreen verification is local evidence; this does not
close a vanilla graphics parity gate or authorize a push.
