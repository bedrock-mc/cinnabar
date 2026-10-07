## crates/client-ui/src/ui_runtime/presentation/forms/oreui/world_settings/; widgets/choice.rs

- Owner-supplied Create New World reference (2026-10-06) shows a neutral80 sidebar, 16:9 forest/river
  preview, native category icons and neutral form sections with recessed fields, primary selected
  choices and secondary unselected choices. Installed hbui `1.26.51.01` is near-version, not the
  pinned `1.26.50` witness.
- `bpe`/`Epe`/`vpe` use `RK.Item`, `RK.ListItem` and `V_` for the preview/actions/seven categories.
  `ype` frames `world-preview-default-d0210bba13d939ca9e72.jpg` at 16:9. `RK` has 1.6rem desktop
  item insets and 4.8rem category rows, with its own vertical viewport.
- General `cfe` wraps world name `rD`, game/difficulty `wX` and Hardcore `O_` in neutral panels.
  `YK` defaults to onRole primary/offRole secondary; selection lowers its face by 0.4rem and draws
  a centered 4.8rem bottom marker. `wX` uses `FX` below 15rem per option; Cinnabar's compact choices
  still wrap instead. `O_` uses `hardcore-heart-engraved-75556ce94d9bfdecca12.png` on its thumb.
- The owner's quick motion and subdued commerce remain intentional deviations. Unimplemented
  categories/Hardcore/Realm creation are disabled, so this visual pass cannot close full parity.

# Vanilla reference map

## crates/client-ui/src/ui_runtime/presentation/forms/oreui/inbox/; crates/client-ui/src/ui_runtime/presentation/forms/oreui/sidebar.rs
- Installed near-version `1.26.51.01` hbui `W3` uses a 102rem narrow breakpoint, a 0.8rem top spacer, desktop 1/3/7/1 grid columns and narrow 0/2/6 columns. `c3`/`i3` select `RK.ListItem` and `V_`: neutral80 sidebar, 1.6rem top/bottom spacing, 4.8rem rows, native category icons and selected icon highlight. CSS `d3b4fa33c4466e32479a` owns the sidebar's 0.2rem border; its ListItem states are indexed in the Settings sidebar entry below.
- `R3`/`A3`/`L3` select category-specific empty cards. `RO` is neutral80 with a 0.2rem border and 1.6rem padding; `AO` uses centered secondaryButton type, `NO` adds 1.6rem above and below the illustration, and `LO` uses centered dimmest captionShort. CSS `c4efde010b75d43947fa` displays the five 128×48 Inbox_No* PNGs at 256×96 times base1Scale. English strings are `data/resource_packs/oreui/texts/en_US.lang` under `hbui.InboxRoute`.
- `W3` opens `/inbox-settings` from the native filter navigation button. `j3` owns a separate settings route with secondary Mark all as read and destructive Delete all read messages buttons. Cinnabar currently exposes those existing maintenance actions; invite preference toggles, subscriber-specific promotions and rich message templates remain incomplete. The owner's quick transitions and quiet commerce treatment are custom extensions, not native animation/parity evidence.


## crates/launcher/src/dressing_room.rs; app/src/player_skin.rs; app/src/player_skin/catalog.rs; app/src/menu/dressing_room.rs; crates/protocol/src/skin_change.rs
- Runtime default choices are declared by installed `data/skin_packs/vanilla/skins.json`: free entries select their original PNG and either `geometry.humanoid.custom` or `geometry.humanoid.customSlim`. The `Dummy` custom slot is not an owned default skin. The installed pack's `geometry.json` supplies the complete source and inheritance; PNGs and geometry remain runtime assets.
- Classic imported PNGs use the same declared wide/slim geometry roots. The requested starter roster exposes Steve and Alex only. Private immutable imports, rename/delete modals and the custom Dressing Room gallery are Cinnabar UI flows; this does not close a vanilla Dressing Room layout/parity gate.
- Versioned `PlayerSkinPacket` carries UUID, serialized image, resource patch, geometry, arm size, cape image/identity and localized names. The live update replaces the retained local profile appearance without replacing roster identity. Login resource patches select the same arm model as the active upload, and ClientData includes the same cape bytes/dimensions/identity.
- Cape upload sizes reuse the existing versioned SerializedSkin normalization: 64×32, 128×64, 256×128 and 1024×512. Imported capes retain source texels, including alpha; skin-body alpha rules do not apply to capes. The custom cape gallery persists its selection independently of the body skin and treats no cape as an absent image.
- Runtime cape sources are indexed under `app/src/player_skin/catalog/default_capes.rs` below; the custom cross-edition wardrobe does not close native Bedrock gallery parity.

Agent cross-reference index: for each file, the vanilla symbols and addresses its behaviour was matched against, removed from source comments by #126. Use it to locate the matching vanilla code; keep source comments free of these references. Entries go stale as code moves.

## app/src/asset_startup/oreui_fonts.rs; crates/client-ui/src/ui_runtime/oreui_fonts.rs; crates/pack-compiler/src/font/outline/runtime/
- Installed hbui `index-800b52fb984b5ed54515.css` uses `Minecraft Seven v2`/`Minecraft Ten v2` followed by the locale-specific Noto family list. Its font-face declarations resolve Merged, SC, TC, JP, KR, Arabic, Mongolian, Syriac and TamilSupplement sources. `index-168bae443ec79c00823c.js` prioritizes JP for Japanese, KR for Korean, TC for traditional Chinese, SC for simplified Chinese and Arabic for Arabic; other locales begin with the merged face. The current `1.26.50.26` reconstruction names `SmoothFontWithHangulFallback` in `src/__unmapped/01.cpp`; `reference/26.30/src/by-owner/f/FontRepository.cpp` records default-face selection. Font tables, rather than handpicked Unicode substitutions, decide glyph coverage.
- The near-version `1.26.51.01` hbui CSS `@font-face` declarations select Seven v2 and Ten v2 OTF, Seven v4 and Five v3 TTF, and Five v2 regular/bold OTF. Native headings use Ten; body uses Seven at 1.6rem/2rem and descriptions at 1.4rem/2rem; ordinary tracking is 0.04rem. The font files' hhea/hmtx tables supply ascent/descent and fractional advances.
- Seven's GPOS kern lookup changes `ra` by -100 and `Fa` by -200 units at 1000 units/em; all DFLT/cyrl/grek/latn kern features share lookup 0. Core OTF pairs use first-glyph horizontal advance only, with pair positioning formats 1 and 2.
- Gameface's [font documentation](https://docs.coherent-labs.com/cpp-gameface/content_development/fonts_frontend/) states that HarfBuzz uses GPOS kerning when available. Its CSS support table accepts letter-spacing and has no font-kerning override. Applying those default pairs together with native tracking is an inference from the host contract and the unmodified native font tables. Full glyph substitution/script shaping and native SDF pixel comparison remain open.
- Current `1.26.50.26` `OreUI::Library::_initializeSystem` passes `--disableSDFonGPU` (`src/__unmapped/05.cpp:1032845`). The near-version native pack `data/shaders/glsl/hb_text_sdf.fragment` samples the red distance channel linearly, decodes `sample*7.96875-3.984375`, and smoothsteps around zero with threshold `(128/255)/Additional.z`. Both `hb_text_sdf.fragment` and `hb_text.fragment` apply exponent `1.45-dot(rgb,[.2126,.7152,.0722])`; raster text uses nearest texel fetch when texture size is available. `data/resource_packs/vanilla/materials/gameface.material` registers these text paths.
- Installed `1.26.51.01` Mach-O retains Renoir symbols. `TypefaceImpl::SDF_BASE_CHAR_SIZE` at `0x1112ff85c` is 52 and `SDF_TEXT_SIZE_THRESHOLD` at `0x1112ff858` is 10. `FT_New_Library` (`0x100a56e48`) initializes embedded FreeType 2.13.2. CPU glyph creation (`0x10015d51c`, called from named `GetOrCreateSDFonCPUGlyphImages`) uses 52 px, FreeType no-hint/no-bitmap flags 10 and normal gray rendering, then pads 4 px on every side. `GenerateDFFromA8` (`0x100124274`) and `GenerateDistanceFieldFromImage` (`0x100123ca0`) initialize weighted coverage gradients with diagonal 1/middle √2 and epsilon .001, propagate edge vectors in forward/backward sweeps, and encode `trunc(128+32*signedDistance)` with saturation outside ±4. The pinned bundled freetype-sys 0.23.0 source uses upstream 2.13.2; native FreeType modifications and an atlas/frame comparison remain a separate acceptance witness.
- `CommandProcessor::SplitCollections` (`0x1000a10d0`) writes CPU type 1's `Additional.z` as requested size divided by 52; `GeometryGenerator::GenerateRectsForGlyphsCollectionScratchGPUData` (`0x1000cc738`) copies it into every vertex. UV derivatives therefore recover its inverse for axis-aligned text in the native pixel coordinate system. The `.41→.27` heuristic in `GetDistanceFieldDeltaPerPixel` belongs to GPU type 2/3 and is not used by the disabled-GPU CPU SDF shader.
- `GetFontDescriptionFromRule` (`0x1007b0e40`) defaults to Auto; `FontManager::RegisterFontForLoadedResource` (`0x100728634`) maps Auto to Typeface policy 2. `GetFontRenderingType` (`0x10015bf18`) selects raster strictly below 10 px, retaining CPU SDF at exactly 10. Raster creation (`0x10015bfcc`, size conversion at `0x10015c240`) truncates the positive requested physical size before `FT_Set_Pixel_Sizes`, loads flags 10 and renders normal gray coverage. Runtime Auto families retain exact raster sizes 1–9; alias selection uses physical pixel size and a distinct cache identity. Explicit raster Seven v4 body/input/item roles are 1.6 rem; Five v3 keyboard labels are 1 rem in `gameplay-580ca3647ed30f2abdb2.css` and `index-800b52fb984b5ed54515.css`. Their exact used raster sizes follow the existing desktop GUI scale range, alongside the small-size variants. Fractional raster geometry and arbitrary future pixel-role sizes remain separate acceptance checks.
- All six installed faces omit U+FFFD and contain an inked glyph 0 `.notdef`; native compilation uses that font-owned missing-glyph outline and metrics rather than the HUD's synthetic replacement. FreeType bitmap top bearings point upward, while the UI adds its owned bearing to a down-Y baseline, so native bitmap top is negated and SDF top padding subtracts 4. The ordinary HUD compiler and its existing minimum-height synthetic replacement remain unchanged.
- Seven v2 GSUB offers stylistic alternates and has no liga feature. Ten/Five v2 liga maps FI/FL case combinations. Current English Settings title/section headings contain none of those pairs, and shared modal titles currently select Seven body; complex-script shaping remains open independently of the English Settings coverage path.

## app/src/menu/focus.rs; app/src/menu/focus/settings.rs; crates/client-ui/src/ui_runtime/presentation/forms/oreui/focus.rs
- Near-version `1.26.51.01` hbui `gn` dispatches directions to a focused node's optional callbacks before invoking visual neighbor search `Ea`; Select invokes a separate `G.A` shortcut. Inline `wX`/`kX`/`PX`/`YK` choices have distinct aliases and no directional callbacks, so arrows move focus and activation commits. Only the responsive `FX` picker explicitly requests selected-item entry focus.
- `Ea` compares full `getBoundingClientRect` bounds, using directional edge gaps, asymmetric overlap fractions, logit penalties capped at 10000/7500, and a 75-degree angular cutoff. Its perpendicular anchor persists across directions on one axis; changing axes or imperative/pointer focus resets it. Matching scroll-axis descendants receive a 0.000001 score multiplier; equal scores retain registration order, and no candidate leaves focus unchanged. Settings supplies no grid-navigation parameters.
- `em` delegates through enabled remembered children, then an explicit target/landmark alias, then the first enabled DOM descendant. Foreign landmark descendants are excluded from direct neighbor comparison; the landmark rectangle wins first and then delegates. `$N` supplies screen/header/content landmarks; `RK` supplies sidebar memory plus selected-category alias; `xde` and `Ide` supply detail and per-tab scroll landmarks. `X_` retains visited tabs with `display:none`, so their focus memory survives category changes.
- `Ug` binds picker focus and supplies memory plus the selected-item alias. Its `gm` header is a separate ordinary landmark; `Ug.Content` disables focus control on both `ap` wrapper landmarks while retaining the inner scroll axis. Those wrappers affect scrolling but do not intercept delegation or remembered item identity. Picker unmounting expires its subtree memory.
- Slider `cue`/`oue` Select toggles adjustment mode without changing the option. Left/Right change a step only while selected and consume input at either endpoint; Up/Down navigate spatially. Back or losing focus clears selection. Switches expose Select only, and each activation reads the current option before generating its inverse target.
- Native key maps `ye`/`Se` bind Select to Enter/Space and Back to Escape/Backspace (`index-168bae443ec79c00823c.js`, UTF-8 offsets 215679/216509). `Gn` selects the FullKeyboard variant, which additionally binds Q; keyboard-type selection remains a separate host integration. `hu` Select dispatch at 344818 and `O_` Switch callback at 716174 preserve one element identity and read its latest value/disabled facets when clicked.
- Slider `cue` registers `Ns` on its inset `sliderRef` wrapper: height 3.2rem and horizontal margins 1.6rem. The moving thumb alone owns hover entry/exit and pointer focus; rail clicks change the value without causing thumb hover or focus. `due` leaves its enclosing `qp` non-interactive.
- Switch `O_` requests the enclosing `qp` focus frame only when disabled and `tr()` is true. The narration provider `Sc`/`bc` derives `tr()` from the host's `core.screenReader.isUITextToSpeechEnabled`; enabled switches show only `P_`'s inherited thumb outline. Cinnabar currently reports no platform TTS support, so disabled narration-only focus still lacks the corresponding host capability.

## crates/client-ui/src/ui_runtime/presentation/forms/oreui/settings/; crates/client-ui/src/ui_runtime/presentation/forms/oreui/transitions.rs
- The near-version hbui BaseSwitch classes keep both rail glyphs mounted. Their interactive click enables a 250ms step-start timeline with the native animated margins and final static snap; unmodified externally supplied values mount statically. Slider thumb/overlay transitions use 300ms cubic-bezier(.39,1.34,.66,1.02), CSS reversal shortening, and overshoot; active mouse dragging disables transition while touch dragging retains it. Track clicks do not capture; the visible thumb retains unrounded drag fractions until release restores the saved step.
- The shared `hu`/`G.A` input driver retains keyboard/gamepad pressed state for 150ms after keydown/onPress. Mouse and touch callbacks commit on release click. Switch focus identities survive inverse target values; source arrow handlers belong to sliders rather than switches. All source timelines use the existing scene clock and retire when their controls unmount.
- Current 1.26.50.26 settings factories in `mcsrc-1.26.50/current/1.26.50.26/src/__unmapped/08.cpp`: accessibility `0x08381f60`, account `0x08385580`. Accessibility orders text-to-speech, gameplay, and user-interface sections; each control reads the option's label, description, value, and state.
- Current video registration `0x00c0d130` and root groups `0x00ccf7e0` in `src/__unmapped/00.cpp`: General, Graphics/performance/layout, View customization, Accessibility: Video. Group factories `0x00cc8610`, `0x00cca7c0`, `0x00ccdb30`, and `0x00cce650`; ordinary graphics groups share `0x00cc20c0` (Fancy callback `0x00cc48f0`). The recovered `VideoSettingsFactoryDetails::createUiScaleModifier` label on `0x00cce650` is a navigation overlay, not a complete description of that body.
- Current sidebar controls `0x049df6c0` and general categories `0x049dff60` in `src/__unmapped/04.cpp`; visibility follows input capability and setting state.
- Native PlayCover `1.26.51.01` hbui bundle is a near-version layout witness, not a matching-version acceptance artifact: `data/gui/dist/hbui/index-168bae443ec79c00823c.js`, `index-800b52fb984b5ed54515.css`, and `menus-theme-b1a483c329eea7188853.css`. Settings route component `Bde`, side menu `RK`, panels `qp`, switch `O_`/`P_`, slider `due`/`oue`, and common grid `yO` identify shell geometry and widget behavior.
- Bundle settings shell uses the global centered title, separate scroll views, a 4/12 + 8/12 wide grid and 3/8 + 5/8 narrow grid. Ordinary setting rows use the neutral palette; depth-three controls use neutral80 indentation. The sidebar uses OreUI assets such as `accessibility`, `keyboard-mouse`, `controls`, `work-bench`, `painting`, `sound-block`, `account`, `subscriptions`, `chest`, and `storage` loaded at runtime.
- Video graphics-group rows keep their full outer width. `qp`'s base CSS class `b9a5bc43d761ce775811` supplies 2.4rem padding on both horizontal sides; indented `e934527bca2094c1a1ef` only adds top/bottom borders. `Eue`'s `hue` wrapper (`a5c24a33713584f2053f`) changes child top margins without a horizontal inset. Its ordinary `ese` wrapper is a fragment outside UI debug mode, and `Rm` adds no layout element.
- Native localization is split between `data/resource_packs/vanilla/texts/en_US.lang` (`menu.*`/`options.*` descriptors) and `data/resource_packs/oreui/texts/en_US.lang` (`hbui.Settings.*` shell strings).
- Bundle option renderer `_se` opens the modal menu for more than five choices, labels longer than 40 UTF-16 code units, or labels longer than 26 when the Settings content column is narrower than 70rem. `Bde`/`Ude` provide the entire content column as the measured container; `wX` also switches to a picker below 15rem per choice. `FX`/`EX` use the shared `SV`/`CV` modal menu, selected checks, and close without changing the option on overlay, X, or Back.
- Picker rows `bV` use `neutral60` interactive background and its `dimmest` border: #8c8d90 throughout, with default/hovered/pressed faces #58585a/#48494a/#313233. Selection adds the 1.6rem × 1.2rem white check; it does not change the fill. `FX` inserts 0.8rem before the closed control, whereas `kX` inserts 0.4rem before segmented choices.
- Closed picker `EX` uses the procedural `sf` path: the active `pD` semantic collection has no pressable component mapping. Its child height is 4.2rem, outer borders are 0.2rem, and elevated padding-bottom/margin-top are +0.4/-0.4rem, yielding 4.6rem flow allocation and 5rem visible height. Pressing removes that padding and margin, keeping the flow allocation unchanged.
- Settings action rows use `Kse`/`Vse`, whose CSS selects native `pressableElevatedSecondary*` border images independently of semantic component mappings. Default height is 4.8rem, pressed height 4.4rem, minimum width 14rem, label padding 2rem. The wrapper's +0.4rem margin cancels the normal button's -0.4rem margin; pressing descends 0.4rem with unchanged flow. CSS focused artwork follows hovered artwork and therefore wins when both states apply. Compact input-reset `PB` instead uses procedural `sf`: 4.4rem width/flow height, 4rem child face, and the 2.4rem runtime `FN.Reset` icon.
- External action `Kse` inserts `FN.ExternalLink` before its label. Class `e637b7180724adbd0253` contributes 0.8rem right margin; the shared Icon24 contributes 2.4rem. The bundle's alpha mask is `assets/external-link@0.5x.icon-28016636e9d767b57dffe6e45fa749aa.png`; icon, gap and label form the centered button content.
- Interactive primitive `hu` retains virtual focus on pointer input but applies focused CSS only when `isFocused && (isLastInputKeyboard || isLastInputGamepad)`. Its explicit interaction mock can override that predicate. The helpers `bt` and `gt` resolve those last-input facets; mouse/touch focus therefore must remain navigable without drawing the keyboard outline.
- Modality tracker `mo` switches to mouse on button-down/wheel, or after mouse movement exceeds 10 pixels from its stored anchor. Keyboard arrows/Tab switch to keyboard. `Ug.Header` hides the close X when the last input is gamepad; pointer and keyboard retain it. `CV` content margins are -0.2rem at both ends, overlapping the 0.2rem header shadow and panel bottom border.
- Settings `Bde` selects `yO(noGutters=true)`. `yO` retains grid gutters only at the 128rem desktop breakpoint; narrow/tablet use the full width. Grid classes `d9f7fd9db73956916b44`/`e5175c04cc2688edc82d`/`c2f03d5f052a87d1367c` establish 2.4rem desktop outer padding and 0.8rem column padding. The Row caps its own width at 128rem after outer padding; the option-provider ref includes column padding.
- Text selectors `ce8c4be1ddc9476d017f` (label) and `d0a4b3633729acae2e4d` (description) use `fonts/Minecraft-Seven-66398119c2c20ee73019.otf`, sizes 1.6/1.4rem, line height 2rem, and 0.04rem letter spacing. The `sectionHeader` selector `e7a4b308889c682b9fc5` uses `fonts/Minecraft-Ten-ed29a1bbe6a620b83378.otf`, size 1.6rem and line height 2rem. Switch row `c2aa1e8dcf8fc0e0d91d` centers the text stack and control in a minimum 4rem body, plus the panel's 1.2rem top/bottom spacing.
- The Settings global title uses `header5A` (`bb757715d5fe9b10e439`): Ten v2, 2rem font size, 2.4rem line height, and 0.04rem letter spacing. `YK` labels explicitly use `body`: Seven v2, 1.6rem/2rem, with the same letter spacing. Adjacent `YK` borders overlap by 0.2rem; the focused wrapper has z-index 1.
- Native v2 OTFs use 1000 units/em and fractional advances: Seven `A`/`i`/space are 600/200/300 units; Ten `A`/`m`/space are 595/805/245. All four v2 faces have GPOS kern features, including Seven `ra` -100 and `Fa` -200 units. [Gameface font documentation](https://docs.coherent-labs.com/cpp-gameface/content_development/fonts_frontend/) describes default GPOS kerning through HarfBuzz; installed SDK behavior with nonzero letter spacing and native rasterization still require a frame witness.
- Switch `P_`, slider `cue`, segmented control `YK`, and radio `JX` use procedural role surfaces rather than pressable textures. Palette `pD` and disabled inheritance `ll` establish primary/neutral50/secondary values and speculars; `P_` selects `assets/onImage-b40d0be137ba09eb7464.png` and `assets/offImage-cc9095b148d166ec7212.png`. Switch position keyframes `a105b6f3afff8c19e69f`/`fb3f380ff3fe1a7abdde` last 250ms with step-start; slider thumb and overlay use a 0.3s cubic-bezier(0.39,1.34,0.66,1.02), disabled during mouse dragging.
- `YK` classes `f14f117a42c5071db91e`/`d4a323c88c739114b4aa`/`da157b4b552e316102c4` establish the 5.2rem face, 0.2rem borders, 0.4rem unselected shadow, and 0.4rem selected/pressed descent. Selected mark `f0bf371d97d59dd95aaa` is 4.8rem wide and 0.2rem high at the bottom center. `JX` class `dbc5182f1ac677f923f3` rotates a 2rem square with 0.2rem borders by 45 degrees; its 0.8rem checked center has white/e6e8eb/e6e8eb/d0d1d4 quarters before rotation. `RadioBoxCheckedDisabled` exists as CSS but is not attached by `JX`.
- Settings Number renderer maps `showSteps` from a present positive metadata `step`, independently of the number of generated slider values. Slider thumb hover styling follows pointer hover, drag, or gamepad selection; focus separately controls its outline. Current `GuiScaleDataProvider::_updateOptions` (`0x04996960`, `src/__recovered/GuiScaleDataProvider.cpp`) produces percentage-labelled option descriptors with values relative to the optimal scale; `createUiScaleModifier` attaches that provider as an OptionComponent rather than a Number slider.
- The bundle does not specify `box-sizing`; [Gameface's documented layout defaults](https://docs.coherent-labs.com/cpp-gameface/what_is_gfp/htmlfeaturesupport/) establish border-box sizing and flex-shrink 0. Absolute positioning in this engine differs from browsers, so final switch offset calculations still require a native frame witness.

## crates/client-ui/src/ui_runtime/presentation/forms/oreui/settings/sidebar.rs; crates/client-ui/src/ui_runtime/presentation/forms/oreui/settings/account_icon.rs; crates/client-ui/src/ui_runtime/oreui_assets.rs
- Near-version `1.26.51.01` hbui `Sse` maps Settings categories to static 24×24 PNGs: accessibility→accessibility, keyboard-and-mouse→keyboard-mouse, controller→controls, touch→touch, party→party, general→work-bench, video→painting, audio→sound-block, account→account, view_subscriptions→subscriptions, global_resources→chest, storage→storage, language→language, creator→command-block. Controller specifically selects `assets/controls-5b4a0c8bc7ac0539b349.png`; the unrelated 14×14 `controls-a12e1e11fbedb31b0eef.png` is not this category icon. `Sse` starts at UTF-8 offset 1816879 in `index-168bae443ec79c00823c.js`.
- `Cse` (UTF-8 offset 1817211) uses `profile.data.xblProfilePic` only for the Account category when `vanilla.userAccount.isLoggedInWithMicrosoftAccount` is true and the path is nonempty. Otherwise it uses `assets/account-46c198f87391d9c79cf7.png`, the gray person silhouette. `V_` paints the gamerpic with centered cover scaling and pixelated sampling, then overlays a 0.2rem neutral border (#1e1e1f) on the same 2.4rem square; the static fallback has no added border.
- `V_` (UTF-8 offset 725022) overlays `assets/icon-highlight-spritesheet-87ec62988bf89f63558d.png`, a 216×24 image containing nine 24×24 crops. `j_` (723468) uses 500ms, `steps(8)` with the default end position, one iteration, normal direction, and forwards fill. Keyframes `caa327c01ef79259bfeb` move background-position from 0% to 100%; frame `min(floor(8*t/500ms),8)` crops at x=24×frame, y=0. Frames 7 and 8 are transparent. CSS `f1cf63a33a644e24eafe` crops the background with overflow hidden; the overlay sits at the icon's left edge without rotation or additional ancestor clipping.
- The highlight's only trigger is the selected facet: unselected sets display:none and selected sets display:flex. Selection after being hidden restarts it; selecting the already-selected category and hovering do not replay it. Neither `V_` nor `j_` consults Screen Animations. Their CSS has no reduced-motion override.
- `Ni` sets rem=5×guiScale and base1Scale=max(floor(guiScale/2),0.5). `V_` and its highlight use 24×base1Scale physical pixels, rounding down at odd scales. Selected `RK` rows carry select classes, so pressed selectors do not change their fill or bevel. Unselected pressed rows without hover retain the sidebar fill and zero-width borders.
- `qp` (UTF-8 offset 443752) calls `_d` (386456) with interactive=false and left/right=false. Ordinary rows retain neutral base fill #48494a, absolute 0.2rem white10% top and black30% bottom edges across idle/hover/press/focus/disabled. Adjacent rows show a 0.4rem pair; `mue` compact headings (1842115) omit both edges, leaving the preceding row's dark bottom visible. Indented `qp` uses neutral80 fill and 0.2rem solid #1e1e1f top/bottom borders; its explicit border flag adds left/right borders. Edges consume no flow height. CSS padding/border selectors occur at offsets 48443–48980; absolute edge rules at 11871–12128 in `index-800b52fb984b5ed54515.css`.
- `RK.ListItem` (UTF-8 offset 1019185) inherits neutral80. Normal rows have no horizontal border; hover and selected classes `aed28ff95c5f041a25dd`/`a9a2aa9391ba4148dad6` add 0.2rem top and bottom borders. `pD` supplies hover fill #48494a with white10% top/black40% bottom; selected fill #48494a with black40% top/white10% bottom. Selected hover keeps the selected role; an unselected hovered pressed row uses #242425 with black80% top/white10% bottom. These are semantic border colors, not shadows or image skins.
- CSS `ad386353cd5071c72c3a` keeps the row at 4.8rem with 1.6rem horizontal padding (0.8rem narrow), a centered 2.4rem icon, and 0.8rem icon/text gap. Its `kd` outline is 0.2rem white; sidebar overrides keep left/right at zero and top/bottom at -0.2rem, or -0.4rem when selected/hovered. `RK.Divider` has a 4.8rem caption with 0.8rem bottom padding, followed by `rj`'s 0.2rem flow slot: its bottom-anchored absolute child paints two 0.2rem reversed bevel strips, black40% above white10%, extending 0.2rem upward. The row selectors are in `index-800b52fb984b5ed54515.css` at offsets 117532–119107.


## crates/client-ui/src/ui_runtime/presentation/forms/oreui/settings/resources.rs
- The pack-management layout is an owner-requested design exception. Installed near-version
  `1.26.51.01` hbui `Mge`/`Phe` identify the base-pack image as
  `assets/minecraft-texture-pack-4c96be5bfdd5a55edf09.png` (256×256); the pack-list fallback is
  `assets/missing-pack-icon-010c87c773e1a21c8ac7.png` (64×64). `Tx` renders its 7×4 chevron
  alpha masks at 1.4×0.8rem. Buttons reuse the shared native elevated state artwork.

## app/src/menu.rs; app/src/screen_policy.rs; app/src/ui_runtime/presentation/forms/panorama.rs
- Near-version `1.26.51.01` hbui `Bde` uses `r$`, whose `qO` background is the full-screen `neutral50` overlay. The role's explicit color is `rgba(0,0,0,0.5)`; this Settings shell introduces no panorama image. Settings retains the world beneath an in-game pause/death stack, while launcher Settings retains the title background. Pack-authored `render_game_behind` still controls whether that underlying game scene may render.

## Movement and input audit repairs (2026-10-07)

Primary reference: Lens artifact 6, reconstructed client 1.26.50.26. The local
26.30 sources under `~/Coding/go/lunar/refs/mcsrc-1.26.50/reference/26.30`
identify systems; current implementations corroborate the rules below. This
preview build is not an exact retail/platform capture for every supported client.

- `crates/gameplay/src/movement/correction_shape.rs` and
  `physics/prediction_corrections.rs`: `_isValidCorrection` `0x04ae62d0`
  requires a nonzero tick at or above the retained floor, without an upper bound;
  `_onCorrectPlayerMovePredictionPacket` `0x04ae6670` and `_applyCorrectionToTick`
  `0x04ae6100` pass it into frame correction. `ReplayStateComponent::applyFrameCorrection`
  `0x02c2a1f0` and `_applyCorrection` `0x02c29db0` have no distance cutoff.
  Missing ticks attach to the current replay frame; `ActorHistory::addCorrectionToFrame`
  `0x02fadc90` sets history correction bits without initiating a frame rewind.
  `RewindSimulation::handleAdvanceAndRewind` `0x038db020` applies corrections
  before each captured input, and clears dirty bits after replay. A later
  same-frame spatial correction wins. The 26.30 `ClientRewind::_advanceRewindFrameSystem`
  `0x058f3850` identifies current-frame capture; current replay logic corroborates
  the pre-input correction order. MovePlayer's separate teleport distance rule
  remains unchanged.
- `crates/sim/src/simulator/environment.rs`, `collision.rs` and `travel.rs`:
  horizontal travel `0x099cc8b0`, walking `0x099ccb00`, flying `0x099cce30`
  sample material at AABB minimum Y minus `0.1f` (`0x14ffab670`). Landing
  response `0x099c4920` selects collision provenance through `0x0208c6f0`:
  highest qualifying shape center below feet minus `0.2f`, then squared distance
  to the feet-plane center, retaining the first exact tie. Restitution minimum
  downward speed at `0x150344840` is `0.08000011742115021f`.
- `crates/sim/src/simulator/effects.rs`: levitation `0x03233fc0` computes
  `v * 0.8f + (amplifier + 1) * 0.01f`, then vertical drag `0x032150c0`
  multiplies by `0.98f`. Horizontal friction `0x03203a50` clears each component
  at or below float epsilon (`0x14ffab690`) before drag.
- Auto-climb `0x09003d90`, registration `0x09004100`: fresh horizontal collision,
  climbable and non-water/non-gliding admission; the resulting travel flag
  excludes later gravity and vertical drag. Ground/air adapters require their
  respective travel tags; travel sensing `0x09fefcb0` selects lava travel
  separately. Lava adapter `0x0904f160` and body `0x09003f40` additionally
  require navigation capability, excluding ordinary player lava auto-climb.
- `crates/input`, `app/src/semantic_controls`, and gameplay input encoding:
  packet fill `0x070fcfd0` and input update `0x07108cc0` retain independent
  digital, raw button, request and actor transition lanes. Raw jump/sneak edges
  accumulate until packet fill clears their bits with `0xfc3fffff`.
  Sprint predicate `0x0c5b6310` uses `0.70710677f` direction/magnitude admission
  (`0x14feff2ac`) and absolute horizontal displacement components against
  `0.0000499999987f` (`0x1503dcba0`), including the swimming exception.
  Pre-move capture `0x0dc09dc0` copies requested motion from `MoveRequest + 0x3c`
  and pre-move position from `StateVector` into gameplay state `+0x1c` and `+0x28`.
  `TravelMoveRequest` `0x09feefb0` supplies velocity to those request fields;
  `SneakMovement` `0x0c597a70` clips them before the resolver in pipeline
  `0x072b3190`. Sprint pipeline registration `0x072b6020` places the seven-tick
  timer (constructor `0x0c5b0650`, callback `0x0c5b0900`) before sprint request
  and intent processing in stage 6.
  Request setup `0x0c5b1b60` supplies vehicle eligibility at `+0xf` and hunger
  admission at `+0x10`; intent processing checks hunger again on the stop path.
  `StorePreviousClientInput` `0x09fda3f0` captures processed forward input and
  the independent sneak request for the following tick. Sneak intent
  `0x0c581310` does not cancel sprint, and sprint action `0x0c5b8810` plus
  setter `0x0c587750` impose no extra sneak veto.
  Item slowdown callback `0x0dc2b3c0` multiplies the intent axes before the
  sprint stage; `SetMoveCommon` `0x099bf880` and `SetMoveClient` `0x099c0590`
  do not overwrite them. Sneak/crawl slowdown follows intent processing.
- Swift Sneak: equipment `0x037597c0`, enchantment registration `0x0373a940`
  (ID 37), and `SneakingSystem` adapter `0x0c59f5a0` corroborate leggings
  lookup and `min(level * 0.15f + 0.3f, 1.0f)`. Constants are at
  `0x150056088`, `0x14ffab6c8`, and `0x14fea4060`.
  Blindness registration `0x0347a4d0` identifies effect 15; sprint intent
  `0x0c5b6310` applies it only when starting sprint.

## app/src/block_entities/describe.rs
- // Current renderSkull selects the model from the backing block type;

## app/src/block_entities/system.rs
- // Current SkullBlockRenderer supplies BlockSource light at
- // the skull's BlockPos to mob_head's ordinary entity material.

## app/src/block_selection.rs
- /// StairBlock::getOutline deliberately returns a full

## app/src/camera/tests/projection.rs
- // Vanilla getFov uses normalized viewport fractions; bx::mtxProjRh

## app/src/environment.rs
- // Current native ClientLevel creates its clock module;
- // registerWorldClock initializes the
- // daylight clock to zero. StartGame current tick instead initializes
- // LevelData's elapsed tick counter. SetTime buffered
- // during loading is applied by onPlayerReady.

## app/src/environment/atmosphere.rs
- // Ordinary preRenderParameters supplies coefficient1; optional platform
- // Ordinary native renderer supplies flag1 to update.
- // buildImage uses it for both ambient stages around gamma;

## app/src/environment/renderer_clock.rs
- //! LevelRenderer's local tick counter.

## app/src/environment/seasonal_foliage.rs
- /// Bounded per-frame Weather::tick samples, previous/current rain in native order.

## app/src/environment/time_override/tests.rs
- // Current ordinary renderer supplies flag1 to LightTexture
- // update; buildImage applies both ambient stages.

## app/src/environment/weather.rs
- // Native Weather consumers interpolate the previous/current tick states;
- // only ClientLevel::_subTick explicitly requests alpha zero for its rate.
- /// Native Weather+0x4c, independent of the interpolated displayed rain.
- /// Native Weather+0x38, used without interpolation by the sky's rain admission.

## app/src/environment/weather_fog.rs
- //! Native WeatherRenderer fog accumulator.
- /// Called once per LevelRenderer tick, not once per rendered frame and not
- /// gated by doWeatherCycle. Native doRainUpdate reads Weather rain at alpha

## app/src/environment/world_clocks.rs
- //! WorldClock::tick and RegistryClient::tick respectively
- //! gate advancement on the clock's pause state and global doDaylightCycle.
- //! Level::getTime's lookup uses its canonical pre-registered hash
- // Legacy handler compares Level::getTime before calling Level::setTime; identical integer times are not re-anchored.
- // Native compares the integer WorldClock time, rather than resetting the

## app/src/item_use.rs
- //! Follows `ClientInputCallbacks::handleBuildAction`, `GameMode::baseUseItem`,
- //! `GameMode::releaseUsingItem` and `Player::completeUsingItem`; projectiles, food effects and
- //! ammunition stay server-owned.
- // Native CrossbowItem::getMaxUseDuration remains its charge duration
- /// `releaseUsing` checks the offhand for either projectile first, then inventory
- /// arrows, and synthesizes an arrow only in creative (09a157e0).

## app/src/menu/input.rs
- /// as vanilla's `TextEditComponent` shows its caret again after typing.
- // VanillaClientInputMappingFactory uses fixed F1/F8 shortcuts.

## app/src/particles/ambient.rs
- //! Other block animateTick callbacks and unclassified native materials remain unsupported.
- /// Vanilla LevelRenderer::tick calls animateTick once per

## app/src/particles/ambient/portal.rs
- // Native PortalAxis::X (1) selects the north/south effect. Unknown (0)

## app/src/particles/drive.rs
- // CommonGameModeMessenger emits local destruction before a server echo.

## app/src/runtime/visibility.rs
- /// Whether the culler hides the box from `low` to `high` in `dimension`: as vanilla's
- /// `isAABBVisible`, only when the cache matches graph `generation` and every sub-chunk the

## core/catalog/home.go
- Live events removed: `/api/v1.0/config/public` (GatheringServiceGetPublicGatheringsRequestHandler) exists only in the 26.30 Edu reconstruction; it is absent from the iOS 1.26.50.04 binary strings and the 1.26.50.26 Windows reconstruction, and the live service returns 404.

## core/catalog/profile_overview.go
- // Vanilla reference: OreUI J b2, Ik, Rk, xk (docs/profile-parity.md).

## core/proxy/resource_pack_admission.go
- ConnectStageRealm      ConnectStage = "realm"      // RealmsConnectProgressHandler: the Realm lookup
- ConnectStageConnecting ConnectStage = "connecting" // GameServerConnectProgressHandler
- ConnectStagePacks      ConnectStage = "packs"      // ResourcePackProgressHandler

## core/proxy/proxy.go
- The pinned Gophertunnel `minecraft/protocol/packet/player_skin.go` defines `PlayerSkin.UUID` as the UUID used in Login; the server applies it only to that player-list identity. The downstream Rust transport identity can differ from the upstream login identity, so the outgoing relay resolves the canonical UUID through `IdentityData()` on the existing session wrappers. Only UUID is rewritten; the serialized skin/model/cape/animation payload and skin-name fields retain their received values. Missing or malformed upstream identity and server-originated skin packets retain raw forwarding.

## core/proxy/targets.go
- remoteServerNetwork / gophertunnel AddressNetwork: MinecraftGame::joinMultiplayerWithAddress (bool false via joinRemoteServerWithAddress; ConnectionType 1/2/8) -> ClientNetworkSystem::probeTransportLayer (URL list, port 0 => 19132 0x4abc) -> TransportProber::start (3 s "TransportProber::timeout") / _tryNextUrl (GET {url}/v1/join, Method variant index 2, 2xx) -> $_0 continuation: error => _joinMultiplayerAfterTransportLayerDetermined(..., 0 RakNet), success => host replaced by URL, TransportLayer 2 (NetherNet). 26.50 adds an https-only fast path and TofuServerIdentityVerifier for http:// results.
- HTTP signaling: ClientNetherNetConnector::connect (types 1/2/8 build HttpSignalingClient, remote id from HttpSignalingClientAnon::createRandomNetworkID), HttpSignalingClient::SendSignal (POST "{}/v1/join/{}", application/sdp, body = payload after 2nd space; response => "CONNECTRESPONSE <id> <body>", error => ESessionError 0x1a), NetherNet::HttpSignalingServer::onRequest/_handleJoin (GET /v1/join => 200 "OK" in 26.30, JSON status in 26.50; 400 "Missing SDP offer in request body").
- No fallback after selection: NetworkSystem::onOutgoingConnectionFailed only notifies; RemoteConnectorComposite::getActiveConnector picks NetherNetConnector iff session transport == 2.
- Transfers: WorldTransferInitiator::initiateTransferToServer builds ConnectionType 8 -> WorldTransferHandler::handleTransferToServer -> ClientInstance::startExternalNetworkWorld("transferServer"), the Play-screen external-server entry.
- Undecodable batches are dropped, not fatal (gophertunnel ErrBatchDropped): CompressedNetworkPeer::_receivePacket returns DataStatus 2 for an unknown header byte or a zlib/snappy failure; 26.50 FUN_1404b0b50 also returns 2 when the byte is neither 0xff nor the negotiated algorithm. NetworkSystem::runEvents::$_0 (26.30) and FUN_1418a075c's loop (26.50) treat any non-zero receivePacket status as "stop this connection for the tick", with no disconnect.
- Server trust (core/proxy/server_trust.go, gophertunnel FirstUseTrust, oreui modal::server_trust_modal): FUN_1408bf550 (probe continuation) builds TofuServerIdentityVerifier (FUN_141190820/FUN_141190a20) with a callback, capturing the probed URL (the modal's %1$s); the callback FUN_1408c07a0 trusts "https://" URLs at once and otherwise pushes ServerTrustModalScreenController (FUN_1408c0c20, modal FUN_145501670: permissions.servertrust.title/message/button.trust/button.doNotTrust). TofuServerIdentityVerifier::verify (FUN_141190bb0) gets the a=identity `assertion` (empty when absent: FUN_1418cf960 returns nullopt, so verify is false) and parses {"fingerprints","token"}, taking the key from the token's cpk; known keys hit a sorted set (FUN_1411eda10 equal_range) and move to the end of the LRU vector, persisted by FUN_141190050 as {"keys":[...]} under the static key "trusted_server_public_keys" (loader FUN_14118f120); FUN_14118f8e0 inserts and evicts past 0xc80 bytes (100 keys). Negotiator side: FUN_140e246d0 hands FUN_140e0fd40's parsed a=identity (or none) to the verifier; a false result logs "Rejecting answer from %s: application declined the server identity".
- No Minecraft-layer AES over NetherNet: EncryptedNetworkPeer::enableEncryption returns early when the inner peer isEncrypted() (WebRTCNetworkPeer::isEncrypted returns true).
- // the Login's multiplayer token and key as the SDP identity, as vanilla's MinecraftIdentityAssertion does.
- // transport accepts identityless answers like vanilla's ClientNegotiator::onRemoteAnswer, while

## core/store/client_test.go
- // Authored to the reference client's inventory parser; not a captured payload.

## crates/assets/src/biome.rs
- // whose constructor defaults surfaceOpacity to .65. Loading that component
- // replaces alpha with its surfaceOpacity.
- // Native LeavesBlock::getRenderLayer:
- // snow eligibility and palette blend strength are independent. The

## crates/assets/src/biome_noise.rs
- /// Converts the full unsigned word to a float as Core::Random::nextFloat does.

## crates/assets/src/block_names.rs
- /// Current SkullBlock identities mapped to the legacy `SkullType` ordinal.

## crates/assets/src/entity.rs
- /// Geometry::_parseBones and the geometry 1.21 schema agree.

## crates/assets/src/gui_item.rs
- /// Static shield ModelPart geometry through its own native GUI matrix, in design pixels.
- /// ModelPart box UV layout, or explicit authored face UV dimensions.

## crates/assets/src/model.rs
- /// SeasonsAgnosticLeaves uses the same cutout/deep group layout, without a

## crates/assets/src/registry.rs
- /// Native BlockReplaceableComponent admission used by the seasonal scan.

## crates/assets/src/seasonal_foliage.rs
- //! 1.26.50.26 `SeasonsRenderer` palette generation
- //! creates covered evergreen/birch/default columns, then their exposed
- /// LeavesBlock::getRenderLayer cold limit.
- /// Native ClientLeavesSeasonColorUtils
- /// skips the main block's air/leaves properties before canBeBuiltOver.
- // canBeBuiltOver predicate: an extra leaf still needs replacement.

## crates/assets/src/sound_events.rs
- // Native BlockGraphics loads textures and `sound` from the same

## crates/assets/src/texture/legacy_terrain.rs
- /// TextureAtlas::updateTextureAtUVs samples a
- /// `2^level` square, normalizes bytes, averages all four channels equally and

## crates/bridge/src/account.rs
- /// The four Xbox title statistics requested by vanilla's PlayerStatisticsFacet.

## crates/chunk-pipeline/src/stream/cohort.rs
- // ClientLoadingProgressTickingSystem::mChunksNeededForLoadOffsets covers nine columns.

## crates/chunk-pipeline/src/stream/dimension_transfer.rs
- // Current client offset initializer 02ddc3a0 copies these 57 eight-byte
- // ChunkPos entries into a 0x1c8-byte vector. Level slot 0x830 (01313e50)
- // returns this list independently of the server's simulation radius.

## crates/chunk-pipeline/src/stream/meshing/types.rs
- /// Vanilla `BlockSource` reads an absent chunk at the dimension's default brightness

## crates/chunk-pipeline/src/stream/residency.rs
- // longer covers (`NetworkChunkSubscriber::moveRegion`), so overlap stays presented.

## crates/chunk-pipeline/src/stream/seasonal_foliage.rs
- //! ClientLevel::_subTick season rows.
- /// SeasonsRenderer::tick refreshes its palette at tick 0 and every hundred ticks.

## crates/chunk-pipeline/tests/it/entity_runtime/pose_defaults.rs
- //! Native BoneOrientation defaults are part of Molang `this`, not animation deltas.

## crates/client-presentation/src/actor_publication/hand.rs
- // The native first-person ActorRenderer root retains its 1/128-model-unit lift
- /// The arm's state at `partial_tick` between the rig's last two ticks, as `renderFirstPerson`
- /// interpolates it: the swing wraps forward past its end, and an eat or drink use of

## crates/client-presentation/src/audio/predicted.rs
- /// Seconds between block hit sounds while mining (`GameMode` 200 ms).
- /// `GameMode` spaces mining hit sounds 200 ms apart.

## crates/client-presentation/src/camera.rs
- /// Native `getNormalizedViewportSize` measures viewport fractions of the full

## crates/client-presentation/src/camera/bob.rs
- //! Walk view-bob and first-person hand sway, expressed as view-space effects, following the
- //! 26.30 reference's bobView and hand spring.

## crates/client-presentation/src/presentation/equipment/display.rs
- /// `ItemInHandRenderer::_applyDefaultItemTransforms` for a flat sprite in hand: the 1.5 scale
- /// Camera-space placement of the first-person held item, from `renderFirstPerson`'s own item
- /// Items whose icon vanilla turns half a revolution in first person (`isMirroredArt`).

## crates/client-presentation/src/presentation/equipment/first_person.rs
- /// Native TextureTessellator pixel-to-model conversion (current PE VA14ffa90e0).
- /// Native renderOffhandItem, not the main-hand swing stack. Blocks use
- // TextureTessellator writes positive column X, depth Y,

## crates/client-presentation/src/presentation/equipment/runtime.rs
- /// by `renderFirstPerson`'s own transforms for the arm's `hand` state.

## crates/client-presentation/src/presentation/equipment/runtime/modern.rs
- /// Native setupAttachableNoChecks copies the parent's complete matrix before the held

## crates/client-ui/src/sound_requests.rs
- /// `min_seconds_between_plays` (`SoundComponent`).

## crates/client-ui/src/ui_runtime/forms/engine_input.rs
- // InputComponent sends pointer deltas to active components;
- // ScrollViewComponent consumes them while capture is still down.

## crates/client-ui/src/ui_runtime/gameplay_authority.rs
- /// Bedrock's GuiData tick notices slot changes even between identical items.

## crates/client-ui/src/ui_runtime/inventory_actions.rs
- /// slot clicked with nothing held, as `CrafterScreenController::handleEvent`.

## crates/client-ui/src/ui_runtime/item_facts.rs
- /// Whether a stack glints as `Item::isGlint` decides: an `ench` list, the item's glint
- /// The format code and colour `Item::getHoverTextColor` gives a component item's name: its

## crates/client-ui/src/ui_runtime/presentation/forms/container_kinds.rs
- // Native ChestContainerManagerModel::_postInit
- // uses the container helper's size, rather than requiring exactly 27 or 54.

## crates/client-ui/src/ui_runtime/presentation/forms/engine.rs
- // An unresolved texture draws `mce::TexturePtr`'s default white texture.

## crates/client-ui/src/ui_runtime/presentation/forms/engine/fill_renderers.rs
- //! (1.26.50 `ProgressBarRenderer`) and `gradient_renderer` (`GradientRenderer`).

## crates/client-ui/src/ui_runtime/presentation/forms/engine/menu_renderers.rs
- //! name tag, after `SplashTextRenderer`, `PaperDollRenderer` and `NameTagRenderer`.
- /// Name tag backing: `BaseActorRenderer::NAME_TAG_BACKGROUND_COLOR`, black at alpha 0.25.

## crates/client-ui/src/ui_runtime/presentation/forms/engine/text_paint.rs
- //! Label painting after vanilla's `TextComponent`: one layout per label with

## crates/client-ui/src/ui_runtime/presentation/forms/engine/tooltip.rs
- //! Native HoverTextRenderer geometry, painted through retained JSON-UI nodes.
- // BitmapFont::getWrapHeight returns default scale × 10.
- // Font::getLineLength rounds the widest line upward first.
- // Native drawCached receives false for its shadow/outline switches.

## crates/client-ui/src/ui_runtime/presentation/forms/hud.rs
- /// Java sidebar background opacities (`getBackgroundColor(0.3)` / `(0.4)`).

## crates/client-ui/src/ui_runtime/presentation/forms/join_progress.rs
- /// `SceneFactory::createNetworkProgressScreen`'s screen.
- /// `SceneFactory::createRealmNetworkProgressScreen`'s screen.
- /// `Util::getFilesizeString`: MB to two places under 1 MiB and one above, GB from 1 GiB.

## crates/client-ui/src/ui_runtime/presentation/forms/menu_caret.rs
- //! The launcher text boxes' caret, after vanilla's `TextEditComponent` as the

## crates/client-ui/src/ui_runtime/presentation/forms/menu_screens.rs
- /// Settings selector index vars as 1.26.50's `SettingsScreenController`
- /// assigns them.
- /// `StartMenuScreenController::addStaticScreenVars` for a full-game, non-edu
- /// The pause store button on a third-party server, as `PauseScreenController`
- /// names it: "%s Store" with the server's store name, else the generic "Server".
- /// The static vars `SettingsScreenController` sets for the global settings a

## crates/client-ui/src/ui_runtime/presentation/forms/recipe_book.rs
- /// `CraftingScreenController::addStaticScreenVars`: radio indexes of the tabs

## crates/client-ui/src/ui_runtime/presentation/forms/server_pack.rs
- /// not the pack also replaces the image (`UITextureInfo::_loadNineslice`).

## crates/client-ui/src/ui_runtime/presentation/forms/settings_storage.rs
- /// Uses the binary megabyte and gigabyte units identified in Util::getFilesizeString.

## crates/client-ui/src/ui_runtime/presentation/forms/settings_support.rs
- /// Populates FeedbackPromptController's three bindings on the actual rating prompt.

## crates/client-ui/src/ui_runtime/presentation/forms/sign_editor.rs
- // `SignScreenController::addStaticScreenVars`: the wood's art and edit box.

## crates/client-ui/src/ui_runtime/presentation/forms/toast_screen.rs
- // `ToastScreenController::addStaticScreenVars`.

## crates/client-ui/src/ui_runtime/presentation/item_gui/shield.rs
- //! The shield GUI ModelPart path, not the first-person attachable animation.

## crates/client-ui/src/ui_runtime/presentation/paper_doll.rs
- //! HUD visibility follows HudPlayerRenderer::update.

## crates/client-ui/src/ui_runtime/presentation/player_preview.rs
- Current `1.26.50.26` PaperDoll constructor `FUN_149c8c3d0` maps `rotation="gesture_x"` to mode 2, preserving `starting_rotation` as the initial/current yaw and `camera_tilt_degrees` as a separate view transform. The pinned `v1.26.50.4` pack's `ui/start_screen.json:657–740` and `ui/pause_screen.json:320–380` both use starting rotation 30°, tilt −10°, and an input panel with `gesture_tracking_button="button.turn_doll"`, pressed Select mapping, and button-up first refusal. `ui/persona_SDL.json` uses the same gesture mode for the active appearance viewer.
- Current `FUN_144cb4dc0` publishes pointer deltas only while the tracked button is held; release clears the held flag/deltas. `FUN_149c8cd20` consumes source-3 X delta once, retains cumulative yaw after release, and computes pointer head angles with the Live renderer's atan divisor/pitch multiplier, the paper-doll's distinct yaw multiplier, and a sign flip while facing backward. It has no release recenter. `FUN_149c8e390` maps `#disable_head_follow_mouse` to `variable.should_look_at_target_ui`; only an explicit `#set_target_rotation` starts shortest-angle interpolation and then restores the prior rotation mode. The mouse/controller gains remain unresolved data globals `_DAT_150068e78`/`_DAT_15012d148` multiplied by the renderer-context factor: the matching executable/Lens transfer is unavailable, so a chosen numeric drag gain remains incomplete rather than a verified parity constant.
- The installed near-version `1.26.51.01` `data/skin_packs/vanilla/skins.json` marks Steve/Ari/Kai/Zuri as free `geometry.humanoid.custom` and Alex/Efe/Makena/Noor/Sunny as free `geometry.humanoid.customSlim`, with sibling PNG textures. Dummy is the custom placeholder. `skin_packs/custom/skins.json` starts empty. These runtime manifests establish free catalog/model facts; they do not establish marketplace ownership. The older named `CustomSkinManager::pickCustomSkin` reference delegates asynchronously to the skin repository and is an import-navigation aid, not current-version validation evidence.
- /// UI rendering retains the native ModelPart origin rather than the world feet origin.
- /// renderer's centre minus the pointer in GUI pixels
- /// (`LivePlayerRenderer::_getMousePosition`).
- /// follows `LivePlayerRenderer::render`: body `atan(dx / 40) * 20`, head
- /// renderer (`LivePlayerRenderer::render`) centres the eyes on the control at
- /// (`PaperDollRenderer::_render`) centres the model at `min(w / 20, h / 39)`
- // PaperDollRenderer sets variable.is_paperdoll=1. The vanilla player

## crates/client-ui/src/ui_runtime/presentation/player_preview/equipment.rs
- /// `setupAttachableNoChecks` preserves expression-bound ModelPart defaults:

## crates/client-ui/src/ui_runtime/presentation/publish/item_icons.rs
- //! Stack-aware icons: native CrossbowItem::getAnimationFrame feeds
- //! getIcon, whose nonzero frame N selects crossbow_pulling variant N-1.

## crates/client-ui/src/ui_runtime/presentation/session_icons.rs
- /// Native CrossbowItem::getIcon routes nonzero animation frames to the pulling

## crates/client-ui/src/ui_runtime/scene_stack.rs
- /// The world and its in-world overlays: the native `InGamePlayScreen`.
- /// `InGamePlayScreen`'s overrides of `BaseScreen`: it passes input through (the
- /// Whether the top scene captures the mouse (`currentScreenShouldStealMouse`);

## crates/client-ui/src/ui_runtime/screen_state.rs
- /// How long local toggles override the block entity, as `CrafterScreenController::tick`.

## crates/client-world/src/actor_animation/attachable.rs
- /// Native setupAttachableNoChecks distinguishes expression
- /// bindings from owner-name matches. Only the latter clear the authored default TRS;
- /// applyAnimations restores the former's ModelPart defaults afterward.

## crates/client-world/src/actor_animation/geometry.rs
- // ItemInHandRenderer's constructor starts both offhand observations at zero.

## crates/client-world/src/actor_animation/hud.rs
- /// Vanilla selects that separate component in Actor; the HUD forces
- /// third person before drawing the same actor in HudPlayerRenderer.

## crates/client-world/src/actor_animation/motion.rs
- // FishAnimationSystem tick consumes StateVector velocity in blocks/tick.
- /// FishAnimationComponent survives geometry/controller resets for this actor lifetime.

## crates/client-world/src/actor_animation/pose.rs
- // ModelPart loader uses 24, then the model
- // constructor negates native Y into BoneOrientation default position.
- // `this` reads BoneOrientation, not an animation-only delta. ModelPart's
- // defaults are copied into that orientation before channels add their values.
- // ModelPart uses an authored X/Z frame and a 24-pixel Y origin. A
- // BoneOrientation negates ModelPart's Y before exposing it to Molang.
- // Owner-name binding clears defaults; an explicit expression keeps ModelPart defaults.

## crates/client-world/src/actor_animation/tick.rs
- // Native ItemInHandRenderer::tick: ±0.4 clamp and cached
- // a mob: Actor::getInterpolatedBodyYaw returns 0, while
- // The native updater publishes FishAnimationComponent before pack scripts.

## crates/client-world/src/actor_store.rs
- /// Native StateVector units for tick-driven engine animation components.

## crates/client-world/src/actor_store/dropped.rs
- // ItemRenderer::render, current 1.26.50.26. The random phase belongs to
- // Native ActorRenderDispatcher uses StateVector origin,

## crates/client-world/src/actor_store/hurt.rs
- /// Native StateVector displacement per tick, distinct from query-derived movement speed.
- /// The current hurt came without damage, so it shows no red flash (`SkipRedFlashComponent`).
- // Native Actor::baseTick decrements only positive
- // Actor::handleEntityEvent, case 0x27.

## crates/client-world/src/actor_store/lifecycle.rs
- // `Player::handleMovePlayerPacket`: Reset sets the position directly and

## crates/client-world/src/actor_store/lifecycle/interpolation.rs
- // Native MovementInterpolator tick clears StateVector velocity

## crates/client-world/src/authority/contracts.rs
- /// Total/current factor for the identified native sprint modifier.

## crates/client-world/src/game_mode_capabilities.rs
- // Retained wire evidence, not a mining gate. Native instant destruction uses
- // Actor::isCreative, not the Instabuild ability (see game-mode-updates.md).
- // `Player::_setPlayerGameType` gives any id but survival the base GameMode, which

## crates/gameplay/src/block_use.rs
- /// Trapdoors and levers flip `open_bit` (`TrapDoorBlock::_useTrapDoor`); an

## crates/gameplay/src/item_use.rs
- /// `handleBuildAction` re-arms the next build action this long after an air use.
- /// `TypedClientNetId<ItemStackLegacyRequestIdTag>`'s process-wide counter.
- // Switching away stops the use without a release, as `Player::stopUsingItem`.
- // `completeUsingItem` finishes locally, without a release transaction.
- // CrossbowItem stores its loaded projectile for the next press's pose/action.
- // `baseUseItem` opens a legacy request scope on every air use.
- /// `TypedClientNetId::_generateNext`: even ids from -4 downward, restarting past the range.

## crates/gameplay/src/item_use/classify.rs
- /// `BowItem`/`TridentItem::getMaxUseDuration`.
- /// `CrossbowItem::getMaxUseDuration`: 25 ticks less 5 per Quick Charge level.
- /// Drink duration of `PotionItem` (and `OminousBottleItem`) and the milk `BucketItem`.
- /// `ItemUseSlowdownSystemImpl`'s factor for a use without `minecraft:use_modifiers`.
- /// `EnderpearlItem::getCooldownDuration`.
- /// Food points below full, as `FoodItemComponent::use` requires.
- /// A use's shared cooldown, as `Player::startItemCooldown` records it.
- /// Whether a vanilla hold use is eaten or drunk (`UseAnimation::Eat`/`Drink`), which the

## crates/gameplay/src/item_use/tests.rs
- /// A depleted use completes locally: the client sends nothing (`Player::completeUsingItem`).
- /// Server-owned lobby items still use `baseUseItem`, without a locally predicted hold.
- /// `TypedClientNetId::_generateNext` restarts at -4 once the counter leaves the negative range.

## crates/gameplay/src/movement.rs
- // LocalPlayer::sendInput copies end-of-tick StateVector motion.

## crates/gameplay/src/movement/collision_registries/connected.rs
- //! Connection-state collision boxes from FenceBlock and ThinFenceBlock.

## crates/gameplay/src/movement/collision_registries/flow.rs
- //! Native material/flow cache bindings; geometry flags do not establish them.
- // Material::_setupMaterials current 0x0379bed0: types 0 / 5 / 6.
- // DirtBlock 0x0a7b9820 / GrassBlockBase 0x0712ab00 use type 1.
- // IceBlock current 0x071305c0 chooses types 13 / 23 (both solid).
- // Current StoneBlock 0x0a5b7aa0 / SandBlock 0x08efbad0 use type 23;
- // recovered registerBlock wrappers are 0x0dfb6010 / 0x0dfb97a0.
- // GravelBlock 0x0712be60 also uses type 23; its vtable 0x1502a5290
- // selects the native falling_dust_gravel_particle producer.
- // These identified native classes use the default liquid detection
- // cache (mask 0) and BlockType directional virtual (always true).

## crates/gameplay/src/movement/collision_registries/selection.rs
- /// Visual bounds for plants; `BlockType::clip` picks these independently
- // TopSnowBlock::getVisualShape: full X/Z,
- // DeadBushBlock constructor overrides inherited
- // flower bounds with grass-sized bounds, including maxY=.8.
- // BushBlock uses
- // minXYZ=(0,0,0), maxX=1; ctor literals set maxY=.8 and maxZ=1.
- /// TorchBlock chooses the visual box by `torch_facing_direction`, independently
- Signs: current 1.26.50.26 `SignBlock::getVisualShape` (RVA `0x0bbba0f0`), reached from the sign registration constructor `0x0bbb90c0` and vtable `0x1503be3d0` slot 10. Standing bounds are `(0.25,0,0.25)..(0.75,1,0.75)`; wall facing 2–5 uses Y `0.28125..0.78125` and a `0.125` thickness against its support. `HangingSignBlock::getVisualShape` (`0x0712ce90`, vtable `0x1502a5f20` slot 10) uses full Y and width with `0.375..0.625` thickness, choosing X for facing 4/5 and Z otherwise.
- Cobweb selection: `WebBlock` retains `BlockType::getVisualShape` through its inherited vtable slot 10 (26.30 `0x10ac8d530`); its constructor clears movement collision rather than the full-cell visual AABB. Current registry maps `minecraft:web` as passable with cobweb response; picking consumes visual geometry independently of that empty movement shape.

## crates/gameplay/src/movement/physics/visual_correction.rs
## app/src/movement/runtime_system.rs
## app/src/movement/teleport_ack_wiring_tests/correction_presentation.rs
- Current 1.26.50.26 correction interpolation creation: `0x036c1530`, identified through `MovementCorrectionInterpolationSystem` registration `0x0369ed60` and adapter `0x036c1aa0`. It accumulates the position correction in `DynamicRenderOffsetComponent`, limits length to 4, records direction, and sets speed squared to `0.2 * length_squared`, floored by StateVector speed when the offset Y is nonpositive.
- Both retained render-offset samples remain inside the 4-block radius: creation clamps current and the tick copies that bounded sample into previous. Replayed history in Cinnabar replaces both position endpoints, so compensating offsets must retain that same bound.
- Current interpolation tick: `0x06bb4780`, reached by `ClientRewind::tickCorrectionInterpolation` adapter `0x06bb4a60`. It retains the prior offset, accelerates retained falling Y by `-0.08` when offset Y is positive, reduces offset length by the selected speed, and removes the offset once its remaining squared length is no larger than the step squared. Rendering interpolates previous/current offsets independently of corrected collision and outbound positions.
- The current render-position interpolation (`0x01c35c20`) consumes those retained offset samples at the frame partial tick. Pausing Cinnabar input admission must therefore keep publishing that already interpolated pose; it must not expose a raw authority assignment or advance correction ticks behind the transport fence.

## app/src/interaction_authority.rs
## app/src/interaction_authority/correction_tests.rs
- Current 1.26.50.26 `InGamePlayScreen::_pick(float)` (`0x004f71b0`) calculates its ordinary pick origin through `0x02c405e0` → `0x01c0db40` → unmounted interpolation `0x01c35c20`. That final routine reads `DynamicRenderOffsetComponent` through `0x01cd8870` (type hash `0x68b69ec0`), adds its previous/current offsets to the corresponding StateVector positions, and interpolates them at the frame partial tick. The pick origin then applies the actor's visual eye/riding offset; it does not use the corrected StateVector position alone.
- The shared presented eye is therefore intentional for both selection and the hit data sent by interactions. A correction updates collision/outbound movement authority immediately while the player's visible aim and block pick ease together. Replacing picking with an authority-only origin during that interval would break the current vanilla crosshair contract.

## crates/client-presentation/src/presentation/visibility.rs
## app/src/ui_runtime/presentation/publish.rs
## app/src/runtime/network/actor_publication.rs
## app/src/tests/menu_scene/hud_visibility.rs
- Hide HUD behavior is visible in the issue reporter's paired Cinnabar/vanilla captures for #234: first-person arms/items and actor name labels disappear together. Hide Hand remains an independent preference restored when Hide HUD is turned off.

## crates/client-presentation/src/entity_shadows.rs
- Local-player shadow admission follows the drawn body perspective; the vanilla first-person capture for #221 has no local-player volume shadow. Frozen local body visibility takes precedence over a stale body submission while changing perspective.

## crates/gameplay/src/movement/control_modes.rs
- /// Native SprintTrigger cannot stop an existing sprint while the previous

## crates/gameplay/src/movement/correction_shape.rs
- /// motion as already matching the retained frame (`getAdvanceFrameResult`).
- /// `MovePlayer` (`_onPlayerMovePacketReceived`, 16.0 read from the 26.30 client).
- /// replays from it with motion cleared as `MovePlayerInput` does; anything
- /// Server StateVector motion; `None` keeps the retained velocity.

## crates/gameplay/src/movement/encoding.rs
- // Raw jump-button carriers track the physical button exactly. Native
- // 0x07108cc0 also sets processed up; 0x070fcfd0 sends it as WantUp,
- // which the server's 0x0998fe80 reads independently of JumpDown.

## crates/gameplay/src/movement/integration_tests/simulation.rs
- // Ground friction distinguishes StateVector motion from displacement.

## crates/gameplay/src/movement/locomotion.rs
- // SprintTrigger runs before SwimTrigger and keeps the previous actor

## crates/gameplay/src/movement/locomotion/swimming_trigger.rs
- //! Current SwimTriggerSystem (0x09fd25a0), for unmounted desktop input.
- // PE VAs 0x14feff2ac, 0x14ffab6c8, 0x15013adc8 and 0x14ffd5070.

## crates/gameplay/src/movement/locomotion_tests.rs
- // Current SendPlayerInputPacket 0x070fcfd0 emits processed up/down as
- // WantUp/WantDown. ServerMoveInputHandler 0x0998fe80 reconstructs them
- // directly; raw JumpDown/Ascend alone do not populate these control lanes.
- // CurrentSwimAmount precedes SwimTrigger. The first dry tick still advances

## crates/gameplay/src/movement/physics.rs
- /// End-of-tick StateVector motion sent as PlayerAuthInput.PosDelta.

## crates/gameplay/src/movement/physics/correction.rs
- // MovePlayer changes spatial state without resetting jump input or
- // movement abilities (native MovePlayerInput RVAs 04b046c0/04b047e0).
- // Vanilla's correction input writes both position and StateVector
- // motion into the corrected frame before replaying later inputs.

## crates/gameplay/src/movement/physics/eye.rs
- // VanillaOffsetSystem dispatcher selects the
- // current-game-version 0x3eb33333 drop. The older-version branch is not our target.

## crates/gameplay/src/movement/physics/sprint_retention.rs
- /// Native SprintTrigger skips its stop action while the preceding swimming
- /// flag and this tick's body-water sensing are set (current RVA 0x0c5b6310).

## crates/gameplay/src/movement/physics/timeline.rs
- /// frame as `ReplayStateComponent::applyFrameCorrection` does. Non-finite
- /// frame as `applyFrameCorrection` does. Modes are only ended, never

## crates/gameplay/src/movement/speed_authority.rs
- /// Native LocalPlayer::setSprinting is edge-triggered; Mob adds/removes only
- /// its identified modifier. An attribute packet replaces that modifier set.

## crates/gameplay/src/movement/teleport_ack.rs
- //! Vanilla Player::handleMovePlayerPacket, mode 2,
- //! sets the action that setFromComponent maps to HandledTeleport.

## crates/gameplay/src/survival_mining.rs
- /// above it, it destroys once per block travelled (`GameMode::continueDestroyBlock`).
- // stopDestroyBlock clears the destroy delay.
- /// `ItemStackBase::hurtAndBreak` keeps damage below `Item::getDamageChance`.

## crates/gameplay/src/survival_mining/tests.rs
- /// Only zero hardness breaks on the start tick (`GameMode::startDestroyBlock`);
- /// stopDestroyBlock clears the delay, so a fresh press starts at once.

## crates/inventory/src/inventory_ledger/admission.rs
- // Native LegacyClientNetworkHandler::handle routes a response to the screen manager

## crates/inventory/src/inventory_ledger/crafting.rs
- // Native _makeCreateItemScopeCreative

## crates/inventory/src/inventory_ledger/crafting_close.rs
- //! ContainerManagerController::_closeContainers invokes
- //! _returnToPlayerOrDrop for every return-on-close input and cursor.

## crates/inventory/src/inventory_ledger/crafting_tests.rs
- /// Native _makeCreateItemScopeCreative declares

## crates/inventory/src/inventory_ledger/distribute/live.rs
- //! Incremental splitting, following ContainerManagerController::_handleSplitMultiple;

## crates/inventory/src/inventory_ledger/queue.rs
- // Native tryPushSlotPrediction returns MissingPrediction,
- // not HistoricPrediction, after a newer owner was removed.
- // Native tryPushSlotPrediction and the historic path

## crates/inventory/src/inventory_ledger/registry.rs
- // Native ItemStackBase::matchesItem compares block/aux identity; a

## crates/inventory/src/item_icon.rs
- /// CrossbowItem::getAnimationFrame, including loaded projectile art.

## crates/json-ui/src/anim/def.rs
- //! Animation definitions as `UIAnimationComponent::_createAnimation` reads them:

## crates/json-ui/src/anim/ease.rs
- //! The 32 easing curves of 1.26.50's `Easing` table (`mce::Math::ease*`), in the
- /// An easing curve, in the client's `EasingType` order.

## crates/json-ui/src/anim/paint.rs
- /// `UIAsepriteFlipbook::tick`: the frame whose span holds `ms` into the loop.

## crates/json-ui/src/anim/runtime.rs
- //! `UIAnimationComponent::_animationTick` does and keeping the values they write.
- /// Resources are ready at the first paint (`onResourcesLoaded`).
- /// `UIAnimFlipbook::tick`: at most one frame per tick, the remainder kept.

## crates/json-ui/src/bind.rs
- //! Data binding as the client's `DataBindingComponent` runs it: each control's
- // `SliderComponent::_createSteps` through the slider's own factory.

## crates/json-ui/src/bind/apply.rs
- //! `DataBindingComponent::_bind` for one control: each binding runs when its
- /// An edit box's content binding seeds the text vanilla's TextEditComponent then owns;

## crates/json-ui/src/bind/bag.rs
- //! A control's property bags at creation, as `UIControl::processPropertyBags`
- //! builds them: its own `property_bag` and `property_bag_for_children`, each
- /// A bag literal's members, each through `UIResolvedDef::_evaluate`: a

## crates/json-ui/src/bind/data.rs
- /// binds, as `ScreenController::bindGridSize` does: a `[columns, rows]` array.

## crates/json-ui/src/bind/native.rs
- //! `DataBindingComponent::_updateCustomComponentsPostBinding`: after a binding
- /// `_getDesiredValue<bool>`: only a JSON bool, else `default`.
- /// `_getDesiredValue<float>`: any number or bool, else `default`.
- /// `_getDesiredValue<int>`: only an integral JSON number, else `default`.
- // jsoncpp's `isInt` also takes an integral real in range.
- /// `Json::Value::asInt`: integers truncate to 32 bits, reals cast, bools 0/1.
- /// `_getDesiredValue<std::string>`: only a JSON string, else `default`.
- /// `PropertyBag::get<T>` on the bag itself: a present value of the right type.

## crates/json-ui/src/bind/spec.rs
- //! A control's `bindings` array parsed as `UIControlFactory::
- //! _populateDataBindingComponent` reads it: the binding type and condition, the
- // (`ScreenController::bindGridSize`) every refresh.
- /// `UIResolvedDef::getAsString`: the field through `_evaluate`, so a constant
- /// `UIResolvedDef::getAsBindingType`: absent is global, `none` binds nothing,
- /// `UIResolvedDef::getAsBindingCondition`: an unknown name logs and reads `none`.
- /// `UIResolvedDef::getAsPropetyEvaluation`: a `#name` is one property; a

## crates/json-ui/src/component.rs
- /// `isInteracted`: the press edge for pointer/gamepad, the release for touch.

## crates/json-ui/src/component/dispatch.rs
- //! Raw input through button mappings to components, as
- //! `InputComponent::handleButtonEvent` / `handlePointerLocation` do: controls
- /// Last button state per (control key, mapping index): `lastButtonState`.
- // `isInteracted`: the press edge, or the release on touch.
- // A button losing its hover lets go of its press (`ButtonComponent::receive`).
- /// `_shouldHandlePressedMapping` and the focused/global rules.
- // `GestureComponent`: its button's hold tracks motion; the release zeroes the deltas.
- /// `SliderComponent::receive` for a button event.
- // `_sendHoverScreenEvent` raises a hover mapping in the Up state: never a press.
- /// `_setChecked` plus the radio group's `_updateToggleGroupState`.
- /// The control's index in its named collection (`UIControl::findCollectionIndex`):

## crates/json-ui/src/component/edit.rs
- //! The text edit component (`TextEditComponent`) of an `edit_box`: what the
- //! factory reads, the retained text/caret state, and the character rules of
- //! `handleTextCharEvent` and `_textFitsInControl`.
- /// Seconds between caret blinks (`updateCaretBlink`).
- // `asInt` admits any integral number, else reads zero.
- /// Apply typed `input` (`handleTextCharEvent`): Enter without newlines ends

## crates/json-ui/src/component/selection_wheel.rs
- //! `SelectionWheelComponent`: pointer sectors and component-managed state children.
- //! Lens 26.30 `receive` (0x1024ea3d0), constructor (0x1024e9af0) and
- //! `_updateControlVisibility` (0x1024e9f30), corroborated by the pinned UI definitions.

## crates/json-ui/src/component/slider.rs
- //! The slider component (`SliderComponent`) as the 1.26.50 factory reads it,
- //! and its value arithmetic (`_updateSliderFromPosition`,
- //! `_updateSliderFromStepSize`).
- /// The value after `direction` small steps (`_updateSliderFromStepSize`).
- /// The step marks a step slider's component creates through its factory
- /// (`SliderComponent::_createSteps`): one per inner step, spaced across the

## crates/json-ui/src/component/sound.rs
- //! The sound component (`UIControlFactory::_populateSoundComponent`,
- //! `SoundComponent::receive`): a shorthand sound for every interacted button

## crates/json-ui/src/component/toggle.rs
- //! The toggle component (`ToggleComponent`) and the toggle manager
- //! (`ToggleManagerComponent`), as the 1.26.50 factory reads them.
- /// What a toggle does with a button event reaching it, as
- /// `ToggleComponent::receive`: the new state and whether a click set it, or

## crates/json-ui/src/env.rs
- //! Variable scopes and value evaluation as the vanilla client performs them
- //! (`UIEval::evalVariable`, `UIResolvedDef::_evaluate`). Each control pushes a
- /// `UIResolvedDef::_evaluate`: a string starting with `$` reads that variable

## crates/json-ui/src/expr.rs
- //! the client's `parseLayoutAxis` does: a lower-cased token stream of numbers,

## crates/json-ui/src/hud.rs
- /// A `#rrggbb` tint as the `[r, g, b, a]` array `bindColor` answers; other

## crates/json-ui/src/input.rs
- /// A control's press sound (`SoundComponent`): its `sound_name`, else the first

## crates/json-ui/src/input/focus.rs
- //! A control's focus component (`UIControlFactory::_populateFocusComponent`) and
- //! the focus containers enclosing it (`_populateFocusContainerComponent`).
- // A non-integer precedence reads as zero, as `Json::Value::isInt` gates it.

## crates/json-ui/src/input/mapping.rs
- //! A control's input component as the 1.26.50 factory builds it
- //! (`UIControlFactory::_populateInputComponent`): its button mappings, the

## crates/json-ui/src/input/navigate.rs
- //! Keyboard/gamepad focus movement over hit regions, after the 1.26.50
- //! `FocusManager`: default focus by precedence, identifier overrides, the
- //! directional sweep (`_sweepForControlDirectional`), scroll sections, and
- //! focus-container rules (`_handleFocusContainerLogic`).
- /// How far into its own edge the sweep starts (`_sweepToNextFocusObject`).
- /// `_sweepForControlDirectional`: the nearest candidate ahead of `current`'s

## crates/json-ui/src/label.rs
- //! A `label`'s text component, as vanilla's `TextComponent` reads it: glyph

## crates/json-ui/src/layout/scroll.rs
- //! Scroll views as the client's `ScrollViewComponent` lays them out: the named
- // `_updateScroll` scrolls only with all four references resolved, along

## crates/json-ui/src/layout/size.rs
- //! clamped before anything reads them, as the client's `LayoutVariable::satisfy`
- //! does.
- // Native `LayoutVariable::isSatisfiable` (26.30, 0x1027946f0) includes
- // min/max dependencies before `satisfy` clamps the ordinary size rule.

## crates/json-ui/src/lib.rs
- // `SceneFactory::_createSafeZoneSizeVar` at the desktop defaults (safe
- // zone 1, screen position 0) sizes every buffer zero along its axis.

## crates/json-ui/src/localize.rs
- //! Label localization as the client's `Localization::_get` applies it: text

## crates/json-ui/src/pack.rs
- /// jsoncpp's `asString`: text as is, bools and numbers spelled out, null empty.
- /// `UIModification::_findIndex` over the original elements: a name matches an

## crates/json-ui/src/predicate.rs
- // `getPropertyValue` without a bag yields the name as text.

## crates/json-ui/src/predicate/ops.rs
- //! `UiExpression::evaluate`: a value/operator stack where prefix `+`, `-` and

## crates/json-ui/src/predicate/tests.rs
- // An unbound `$var` is null as in `UIEval::evalVariable`; with no bag a `#name`

## crates/json-ui/src/predicate/token.rs
- //! Expression source to tokens, as `UIEval::evalExpression` splits it and
- //! `ExprToken::_parseToken` types each piece.
- /// Operator codes, as the client's `OperatorType` numbers them.
- /// `Json::Value::asBool`: nonzero, nonempty, or true.
- /// `Json::Value::asInt`: strings and compound values read 0.
- /// `Json::Value::asFloat`: strings and compound values read 0.
- /// `createTokenFromUIDefVal`: the token a JSON value becomes.
- /// `ExprToken::_parseToken`: keyword, quoted string, property, int, float,
- /// `Util::toBool`, case-insensitive: `true`/`false`, `yes`/`no`, `1`/`0`.
- /// `ExprToken::createStringToken` for an operator's text result: reparsed as a

## crates/json-ui/src/scene.rs
- /// A screen root's settings. Defaults are the parser's (`getAsBool` fallbacks).
- /// Read by `UIScene::ignoreAsTop`: the scene below still counts as topmost.
- /// bool (an unbound `$var`) keeps the default, as `getAsBool` does for null.
- /// Scenes in paint order (`forEachVisibleScreen`): from the highest scene that
- /// `ClientInstance::currentScreenShouldStealMouse`.
- /// `ClientInstance::isShowingMenu`.
- /// history alone when it holds none (`popScreensBackToFirstInstanceOf`).

## crates/json-ui/src/sprite.rs
- //! The `image` control's sprite, following vanilla's `SpriteComponent` draw
- //! dispatch: nine-slice first, then a clipped, tiled, filled (cover), kept-ratio

## crates/json-ui/src/widgets.rs
- /// its content sibling's name (`DropdownComponent`).
- /// The content's top as `DropdownComponent::_positionContent` places it:

## crates/json-ui/src/widgets/scroll_motion.rs
- //! Touch scrolling as 1.26.50 `ScrollViewComponent::_updateDynamicsAndScrollPosition`
- //! runs it: a held finger pulls the offset on a spring, a release flings it

## crates/json-ui/src/widgets/states.rs
- //! Which state controls a stateful control shows, as the components'
- //! `_updateControlVisibility` write them: each named target is the first
- /// `ToggleComponent`'s targets by `checked + 4·hover + 8·locked`; unused slots are empty.

## crates/json-ui/tests/it/bind_native.rs
- //! typed readers (`_getDesiredValue`), one case per post-binding target.
- // jsoncpp's `isInt` takes an integral real, as a controller count is.

## crates/json-ui/tests/it/bind_parity.rs
- //! client's `DataBindingComponent`, one case per audited behaviour.
- // B37: `ignoreCollectionItem` keeps a child out of the collection.
- // the text vanilla's TextEditComponent keeps, so typing shows as it happens. Vanilla's

## crates/json-ui/tests/it/layout_parity.rs
- // V08: a horizontally draggable box scrolls the content along x (`_updateScroll`

## crates/json-ui/tests/it/resolution.rs
- // token parses as an int from its leading digits (`Util::toNumber<int>`), so
- // An item index counts only under the collection's own panel; `ignoreCollectionItem` opts out.

## crates/json-ui/tests/it/tooltip.rs
- //! HoverTextRenderer receives its authored maximum width, not just # bindings.

## crates/json-ui/tests/scroll.rs
- //! Scroll views as vanilla's ScrollViewComponent runs them: the named viewport,

## crates/launcher/src/menu/profile.rs
- /// Bedrock truncates the service's minute value before passing it to DateHelper.

## crates/launcher/src/menu/settings_options/chat.rs
- /// Mirrors ChatUtils::canLanguageBeSmooth's four unsupported locales.
- /// Applies ChatUtils' one-decimal padding plus the source's nonzero epsilon.

## crates/launcher/src/menu/settings_options/control_bindings.rs
- // R:v/VanillaClientInputMappingFactory.cpp: key.emote defaults to B.
- // R:26.30 createInputMappingTemplates action0x34 uses native button7;
- // GamePadRemappingLayout's native sprite/name map identifies it as D-pad left.

## crates/launcher/src/menu/settings_options/definitions.rs
- // current OptionRegistry values are recovered; see plan.md.

## crates/launcher/src/menu/settings_options/emotes.rs
- /// R: native EmoteWheelScreenController equipped top/right/bottom/left slots.

## crates/launcher/src/menu/settings_options/keybindings.rs
- // KeyboardRemappingLayout replaces the list with one captured key.

## crates/launcher/src/menu/settings_support.rs
- /// Fixed destinations from general_section.json and AppPlatform::getFeedbackHelpLink.

## crates/meshing/src/chunk/cube_materials.rs
- /// GrassBlock::calcVariant samples only the block directly above:

## crates/meshing/src/chunk/leaves.rs
- //! LeavesBlock::_isDeep tests six neighbours;
- //! BlockOccluder::_updateRenderFace retains only one of the
- // Both native leaf types use this predicate; only ordinary LeavesBlock
- // adds the seasonal colour material flag.

## crates/meshing/src/chunk/seasonal_foliage.rs
- //! ClientLeavesSeasonColorUtils scans upward
- //! to the height map, skipping air/leaves and native exempt blocks. TopSnow

## crates/meshing/src/cloud.rs
- /// Current native TextureTessellator colour bake, before conversion

## crates/meshing/src/cloud_viewport.rs
- //! Current 1.26.50.26 tickClouds rebuilds after fifteen blocks of
- //! floor(samplePosition)>>4, and TextureTessellator emits unit caps

## crates/meshing/src/lighting.rs
- // BlockType::getShadeBrightness:
- // property 0x20 and Block+0x71, independently of Block+0xa3.
- // y+1 plane, rounding each channel. Admission uses BlockType+0x15c > 0.5,
- // AmbientOcclusionCalculator::calculateWithCache
- // averages four independent 0.2/1 shade samples. Its diagonal fallback
- /// PortalBlock takes the native flat path: boundary-adjacent light, own light

## crates/meshing/src/lighting/native_liquid.rs
- // independently of AmbientOcclusionCalculator's terrain sampling.

## crates/meshing/tests/it/mesh/snow_covered.rs
- /// block's own pass (tessellateTopSnowInWorld). A crossed

## crates/meshing/tests/it/support/liquid_contacts.rs
- // BlockType with Air, not its opacity. Deferred model 1 instead compares material;

## crates/pack-compiler/src/animation.rs
- /// Overlay-mask sources (grass sides) use the TextureAtlas::updateTextureAtUVs /
- /// _buildAtlasMips byte-space box mips, as every vanilla atlas tile does.

## crates/pack-compiler/src/compiler/lily_pad_textures.rs
- // TextureAtlas::updateTextureAtUVs multiplies RGB only.

## crates/pack-compiler/src/compiler/seasonal_leaves.rs
- // LeavesBlock::getRenderLayer chooses an
- // opaque deep material without changing getVariant's fancy texture.
- // SeasonsAgnosticLeavesBlock::getRenderLayer
- // also chooses layer5/7 by depth, but never seasonal layer9/10.
- // Exact current BlockReplaceableDescription registration witnesses:

## crates/pack-compiler/src/compiler/visuals/cross.rs
- /// Native row tessellation (MCSRC 06a21df0 / 06a98c10): four full-width

## crates/pack-compiler/src/compiler/visuals/portal_tests.rs
- // Current final PortalBlock registration supplies light, without opacity.

## crates/pack-compiler/src/compiler/visuals/signs.rs
- // Classic vanilla's SignModel declares a 24x12 board and its

## crates/pack-compiler/src/compiler/visuals/snowy_grass.rs
- // by GrassBlock::calcVariant.

## crates/pack-compiler/src/entity/animation/roots.rs
- // ActorResourceDefinitionGroup::upgrade_v1_8_to_v1_10
- // moves legacy controllers into a distinct animation alias before appending activation roots.

## crates/pack-compiler/src/entity/item.rs
- // Vanilla draws an item as its block only when it is that block's own BlockItem: an item
- /// torchflower) are that block's `BlockItem`, drawn from its first canonical state.

## crates/pack-compiler/src/entity/item/spawn_eggs.rs
- //! ActorPlacerItem resolves icons by actor identifier, not item-atlas key spelling.
- //! Current 26.50.26 getIconInfo reads the actor icon map. The
- //! ActorResourceDefinitionGroup loader reads description.spawn_egg
- //! texture/texture_index; ActorPlacerItem::isValidAuxValue accepts only zero.

## crates/pack-compiler/src/entity/item_bindings.rs
- //! Current Item::initClient reads components.minecraft:icon;

## crates/pack-compiler/src/entity/legacy_icons.rs
- //! Legacy vanilla icon routes: the atlas key and variant the retail client's
- //! `VanillaItems::initClientData` assigns to items without an icon component,
- //! with potion icons keyed by aux. Each row cites its call site in the 26.30
- //! client; keys absent from the pinned atlas are skipped, never invented.

## crates/pack-compiler/src/entity/native_bind_pose.rs
- // llama.geo.json has the same bind. Native GeometryGroup keeps same-identifier history;
- // Geometry::_parseBones reads missing bind fields through
- // JsonValueHierarchy::get, retaining the older shipped bind under the modern sample.

## crates/pack-compiler/src/entity/native_dragon_geometry.rs
- // These parts use parent-relative pivots with ModelPart's Y origin.

## crates/pack-compiler/src/icon/blocks.rs
- //! Vanilla's item renderer draws a block item flat when `BlockTessellator::canRender` rejects
- //! its shape; `BlockItem::getIconInfo` then shows the carried texture, down face, at the
- // The world key's variant stands in for the block's `getVariant`.

## crates/pack-compiler/src/icon/carried.rs
- // Matched 26.50 TextureJSONParser keeps the low RGB bytes
- // of the hexadecimal value and forces opacity to one. The matching
- // TextureAtlas::updateTextureAtUVs mixes original/tinted RGB

## crates/pack-compiler/src/icon/shield.rs
- //! ShieldRenderer GUI branch, not the first-person attachable animation or a flat UV sheet.
- // The retail ShieldModel loads its named root. Exotic animated/inherited model-part trees

## crates/pack-compiler/src/pack/block.rs
- /// The texture key vanilla's `BlockItem` icon reads: `carried_textures`, else `textures`,

## crates/pack-compiler/src/pack/fixed_tint.rs
- // TextureJSONParser delegates to colour parsing:

## crates/pack-compiler/tests/it/shield_icon.rs
- // A different legacy atlas image must never override the native ShieldModel branch.

## crates/pack-compiler/tests/it/water_appearance.rs
- //! WaterRenderAttributes retains its alpha when a biome only replaces RGB.
- // Current getWaterColor reads the default RGBA.

## crates/particles/src/ambient.rs
- /// Material::_setupMaterials: air(0) and plant(8)
- /// are neither solid nor liquid. TallGrass, Flower,

## crates/particles/src/ambient/fire_tests.rs
- // Independently rounded witnesses for native Random::nextFloat's double

## crates/particles/src/ambient/random.rs
- /// Current Random::nextFloat converts one unsigned MT word to double,

## crates/particles/src/ambient/sampler.rs
- // Target-version LevelRendererPlayer constructor starts
- // mode 2 at 100 samples with its previous camera position zeroed.

## crates/particles/src/emitter/manual.rs
- /// `LevelRendererPlayer::addBiomeTintedParticleEffect`
- /// keeps one emitter per colour and calls its manual emission method for every origin.

## crates/particles/src/system.rs
- /// `_addTerrainEffect` checks the selected effect's

## crates/particles/src/system/biome_tinted.rs
- /// Emits one biome-tinted particle at a block center. Vanilla 26.50's
- /// `LevelRendererPlayer::addBiomeTintedParticleEffect` caches

## crates/particles/src/tiles.rs
- /// `BlockDestructionParticlesComponent::getTextureInfo`
- /// resolves `down`, then `*`; the built-in texture fallback uses

## crates/particles/src/tiles/tint_tests.rs
- // SeasonsRenderer::getTintedColor clamps doubled palette RGB, whereas
- // RenderChunk's seasonal shader multiplies it into the texture unclamped.

## crates/particles/src/triggers.rs
- /// Default destruction count from vanilla BlockDestructionParticlesComponent.

## crates/protocol/src/actor/status.rs
- // The vanilla constructor and handler pass StateVector origin

## crates/protocol/src/interaction.rs
- /// Builds the click-air transaction vanilla's `GameMode::baseUseItem` sends: zero block and
- // `setPlayerContainer` stamps and records only a non-empty result.
- /// Builds the release-item transaction `GameMode::releaseUsingItem` sends when the use button

## crates/protocol/src/inventory/transaction.rs
- //! Vanilla client verification does not reject a stale fromItem.
- // Native UI output 50 defers an InventoryTransactionManager action;

## crates/protocol/src/item_capacity.rs
- /// The stack size `Item::Item` gives every item until its definition says otherwise
- /// (`mMaxStackSize`, 64): a server block's own `BlockItem` keeps it.

## crates/protocol/src/login.rs
- // Login has already initialized this session's registry. Native 1.26.50
- // ItemRegistry::matchServerItemIds returns once its
- // initialization state is complete, including for an empty/custom-only
- // repeat. Decode the wire first so malformed repeats remain fatal;

## crates/protocol/src/ui.rs
- /// Floors a coordinate as `LevelRendererPlayer::levelEvent` does.

## crates/protocol/src/ui/forms.rs
- // ServerFormBindingInformation::createBindingData
- // normalizes both representations through the same image value. Absent

## crates/protocol/src/world/clocks.rs
- /// The vanilla clock consumed by Level::getTime and the atmosphere renderer.
- /// The vanilla initializer constructs this exact
- /// name; Level::getTime uses the resulting hashed string.
- /// Native HashedString key for the built-in daylight clock. The current
- /// initializer assigns this key alongside OVERWORLD_CLOCK_NAME;
- /// registerWorldClock pre-registers it and Level::getTime's
- /// lookup searches that ID directly, not packet string names.
- /// Native RegistryClient initialization upserts registrations; it does not

## crates/protocol/src/world/events.rs
- /// Native ClientNetworkHandler 014b1c90 stores this phase without moving the actor.
- /// Native ClientNetworkHandler 014b1c90 applies this phase through LocalPlayer::respawn.

## crates/protocol/src/world/game_rules.rs
- /// Native GameRules slot 9 gates ClientLevel's seasonal palette accumulation.

## crates/protocol/tests/it/actors.rs
- // MoveActorDeltaData::parseDeltas merges into the previous absolute data,

## crates/protocol/tests/it/interaction_packets.rs
- /// Air use matches `GameMode::baseUseItem`: action 1, face 255, no trigger, no block.

## crates/protocol/tests/it/item_stack_requests.rs
- /// Current SparseContainerSetListenerClient::postSetItem stamps cells;
- /// ItemStackRequestActionHandler::_validateRequestSlot resolves them.

## crates/render-model/src/actor/bind_pose_tests.rs
- //! Cube-local bind poses from native ModelPart cube setup, not hierarchical bone rotations.

## crates/render-model/src/actor/geometry.rs
- // Native ModelPart cube setup adds the bind Euler angles
- // Geometry::_parseBoxFaceUV first copies the cube's

## crates/render-model/src/actor/texture_mesh.rs
- //! Native attachable raster extrusions (TextureMesh::compileQuads).

## crates/render-model/src/equipment/display.rs
- /// Native `_renderOffHandItem` has its own bone-frame offset and
- /// Legacy block grip, applied to the centred cube emitted by
- /// `_rebuildItem` through the (-.5,-.5,-.5) mesh offset.
- /// Items vanilla holds upright (its `isHandEquipped`): tools, weapons and rod-like items. The

## crates/render/src/actor.rs
- /// axis (`LevelRendererCamera::queueRenderEntities`, `min(radius, 72)`); players are added apart.

## crates/render/src/atmosphere.rs
- /// Cloud RGBA from `DimensionClientUtils::getCloudColor`: weather tint, day brightness,
- // Current DimensionClientUtils, classic non-custom branch:
- /// LevelRenderer::tick drives clouds even while daylight is paused.
- // getInterpolatedSkyColor also mixes precipitation fog before thunder.
- // That stage waits for Weather+0x4c/current-rain view inputs after profile resolution.

## crates/render/src/atmosphere.wgsl
- // LevelRendererCamera rotates the star mesh around +Z.
- // Current 1.26.50.26 buildSkyMesh has red0 at its centre and
- // red1 at this decagon rim. renderSky places its plane at Y256
- // orbit. buildSunAndMoonQuad maps−X→u1 and−Z→v0, hence fixed+Z
- // degrees (moon offset 180). Ordinary renderSunAndMoon admits the
- // Target renderSunAndMoon scales the stock celestial alpha by

## crates/render/src/atmosphere/liquid_distance.rs
- /// Incomplete: the native underwater/no-FrameBuilder branch uses a distinct

## crates/render/src/biome_tint.wgsl
- // interpolating the ordinary foliage lattice (FoliageTessellationPolicy).

## crates/render/src/block_entity/crack.rs
- /// Current extractCracks routes through the block

## crates/render/src/celestial.rs
- /// Colour helpers use the native table, unlike getSunDirection's full-precision sinf/cosf.
- /// Ordinary Dimension::getSkyDarken, consumed by builder.
- // DimensionClientUtils::getStarBrightness: weather participates
- /// Current getSunriseColor does not read weather. The sky and cloud

## crates/render/src/chunk/transparent/face_metric.rs
- // Current vanilla RenderChunkSorter perspective sort uses
- // Current CentroidPlusReverseBit packs the emitted-vertex

## crates/render/src/chunk/transparent/gamma_pass.rs
- //! Current client RendererSettings selects UNORM format 0x57, while

## crates/render/src/cloud.wgsl
- // TextureTessellator's exact face sequences preserve both outward

## crates/render/src/cloud_config.rs
- /// Current native recalculateRenderDistance removes a distance-dependent
- /// Vanilla stores packet radius + one chunk in Player::mChunkRadius;
- /// Current tickClouds's feature-disabled Fancy route. Advanced
- /// FrameBuilder quality selection is a separate, still-open parity gate.

## crates/render/src/cloud_render.rs
- // Native cloud PassState cull1 translates to BGFX CULL_CW:

## crates/render/src/dropped_item.rs
- /// Native TextureTessellator frame after the ordinary dropped-item default transform.

## crates/render/src/dropped_item/native.rs
- //! Current 1.26.50.26 ItemRenderer. Item-local
- /// `getRenderYOffset`'s table-index truncation and cubic ease-in are retained. Computing

## crates/render/src/lighting.wgsl
- // Current classic builder stores Color::toABGR, which
- // truncates RGB to bytes before LightTexture::getColorForUV reads them.

## crates/render/src/material.wgsl
- // enabled by BlockGraphics' pack-authored isotropic mask.
- // AmbientOcclusionCalculator raises the four-sample average * face

## crates/render/src/native_sunlight.rs
- /// Weather+0x38: current simulation rain, not the frame's interpolated rain.
- /// getSunIntensity; the caller supplies the narrow or broad threshold.
- /// getInterpolatedSkyColor: precipitation fog first, then thunder.
- /// Overworld::getFogColor; unlike cloud colour, fog uses full cosf.

## crates/render/src/screen_overlay.wgsl
- // Portal: native FullScreenEffectRenderer unit cube and the active atlas flipbook.

## crates/render/src/screen_overlay_portal.rs
- // Current getDestructionParticlesTexture 04e95920 chooses default face zero.

## crates/render/src/weather.rs
- //! Precipitation model after the vanilla `WeatherRenderer`: biome lattice, per-kind intensity,
- /// Weather::tick approaches its targets by this amount.
- /// Per-kind constants from the vanilla `paramsRain`/`paramsSnow` tables.
- /// Biome sample lattice around the player from the vanilla `precipitationOffsets` table.

## crates/render/tests/it/atmosphere.rs
- // Current getCloudColor, legacy (non-custom) branch.
- // Native admission reads Weather+0x38, independently of interpolated frame rain.

## crates/render/tests/it/block_selection_native.rs
- // Named native StairBlock::getOutline deliberately uses one full box.

## crates/render/tests/it/cloud_render/corners.rs
- // Current native TextureTessellator emits these four vertices per

## crates/render/tests/it/native_sky.rs
- /// Current 1.26.50.26 buildSkyMesh stores a black centre and white
- /// decagon rim. renderSky translates it to Y256 and scales it by2000.
- /// Current renderSunAndMoon admits orbital phase through 105/255.

## crates/render/tests/it/skull_lighting.rs
- // Native getColorForUV: /16 coordinates, clamp-linear sampling of RGB bytes.

## crates/sim/src/destroy.rs
- /// `WeaponItem::getDestroySpeed` gives bamboo the harvest divisor as its speed,

## crates/sim/src/math.rs
- /// Native look vector used by the swimming trigger (current RVA 0x09fd25a0).

## crates/sim/src/simulator.rs
- // FinalizeMove uses the native float epsilon.
- /// `bedsim v0.1.3` `ClimbSpeed`, cited there against `Mob::ascendLadder()`.
- // TravelTypeSensing (0x09fefcb0) selects water by WasInWater,
- // Current BedBlock restitution.
- /// `WaterTravelSystem`'s travel speed: the water base blended toward the ground
- Ground contact: FinalizeMove `0x06dcbfc0` compares the move request (+0x30) with the
  result (+0x3c). A vertical difference above float epsilon adds OnGround only when
  request y < 0, otherwise removes it; with no vertical difference OnGround survives only
  if it was already set and request y is exactly 0. AutoStep filter `0x09f5a7f0` admits a
  step on last tick's OnGround, so a jump-tick step rises without grounding.
- Sneak edge avoidance `0x0c597a70`: gated on sneak plus OnGround (no vertical-velocity
  check); probes the actor's current AABB inset 0.025 on x/z and lowered by step height
  times 1.01; clips request x, then z, then both while either is nonzero; zeroes a
  velocity axis (+0x18/+0x20) only when its clipped request is at or below float epsilon.

## crates/sim/src/simulator/collision.rs
- // Like `AutoStepSystem::getMaxCollisionVolume`, cover the raised path too.
- `clip_sneak_edge`: see the sneak edge avoidance entry under simulator.rs (`0x0c597a70`).

## crates/sim/src/simulator/environment.rs
- // Current BlockSource::containsAnyLiquid (0x031a7a20)
- // reads getBlock's primary material, without secondary layers.

## crates/sim/src/simulator/flight.rs
- // HorizontalFlySpeedControl current RVA 0x03235360, PE VA 0x1501672a8.
- // VerticalFlySpeedControl current RVA 0x02c452b0, matching PE float lanes.
- // FlyDrag current RVA 0x03217940 reads this friction coefficient; retention
- // is its native float subtraction from one, independent of horizontal drag.
- /// Flying shares DefaultMoveSystems' ground-friction probe at starting AABB
- /// minimum y minus native float 0.1, including fractional support heights.
- // Current horizontal drag RVA 0x03203a50 clears each lane at its float
- // epsilon before applying friction; this is independent of vertical drag.

## crates/sim/src/simulator/mode.rs
- /// Current Player constructor's SneakingHeightChangeVersion value.
- /// Native horizontal pose uses collision width as height (RVA 0x02c33550).
- /// Native bounding-box input update shrinks all probe faces (RVA 0x09eeeb70).

## crates/sim/src/simulator/state.rs
- /// Native SwimAmountComponent blend retained across ticks and replay. Both
- /// Retained native swimming-or-crawling flag observed by SwimAmount before

## crates/sim/src/simulator/travel.rs
- //! Flight controls and liquid movement follow the current mcsrc client systems.

## crates/sim/src/simulator/water.rs
- //! Current-client liquid drag, jump ascent and swimming pitch steering.
- // Current UnderWaterSensingSystem (0x09fd7c00), PE VAs 0x150064950/0x150167290.
- /// CurrentSwimAmountSystem (0x099e64c0) precedes MobJumpSystem in the
- /// native category's registration order. Crawl flag 114 also advances the blend.
- /// MobJumpSystem (0x0a5dc2e0) suppresses every jump path while the blend
- // 1.26.50.26 RVA 0x0320fc20; PE VAs 0x14ff9c370 and 0x15005ea28.
- // MobJumpSystem equivalent 0x0a5dc2e0 reads PE VA 0x1500b5374.
- // WaterSinkInputSystem equivalent 0x0dc3db30 reads PE VA 0x150106adc.
- // SwimControl equivalent 0x09fd2140; PE VAs 0x150361800 and 0x14ffab668.
- // MobMovementClimbOutOfLiquid 0x09004df0; PE VA 0x14ffab698.
- /// Swimming pitch steering runs only without a held jump. The native dispatch
- /// excludes MobIsJumpingFlagComponent and lets MobJumpSystem handle ascent.
- // RVA 0x09fd2140: ordinary upward steering requires the liquid material
- // flag written by bounding-box input update, even if velocity was falling.

## crates/sim/src/world/current.rs
- //! Player liquid-current impulse from the preceding pose (native 0x0a5d5c40).
- // Native Range<int> uses floor(f32(upper + 1)) as its exclusive
- // LiquidPhysics selects lava when both material probes find contact.
- // Matching PE preserves (X² + Y²) + Z² despite the reconstructed
- // expression listing the operands in another order.

## crates/sim/src/world/flow.rs
- //! Native liquid-cell direction (LiquidBlockBase::_getFlow, current 0x0395d2f0).
- // Facing::PLANAR at PE 0x15013e0c7; face masks are 1 << Facing.
- /// Source-identified material and liquid detection facts, independent of meshes.
- /// `blocked_faces` is BlockLiquidDetectionComponent's cache byte at Block+0xb9.
- /// `allowed_faces` records the native directional virtual's admitted faces.
- // getLiquidBlock (+0x28): extra layer unless it is air, then primary.
- // Matching PE 14395d72d / 14395d793: (X² + Y²) + Z².

## crates/sim/src/world/liquid_probe.rs
- /// Native LiquidPhysics material probe, using the preceding collision pose.

## crates/sim/tests/it/embedment_convergence.rs
- // FinalizeMove reconstructs the centre of the native float AABB.

## crates/sim/tests/it/flight_native.rs
- //! Frozen float regressions from identified current-client flight controls and
- //! PE constants. These are source-derived cases, not captured live trajectories.
- // DefaultMoveSystems flying wrapper 0x099cce30 calls horizontalMovement
- // 0x099cc8b0, which samples starting feet minus 0.1 before collision.
- // VerticalFlySpeedControl 0x02c452b0 precedes movement; FlyDrag
- // 0x03217940 retains y independently. Each pair is (movement, velocity).
- // HorizontalFlySpeedControl 0x03235360 reads float ability 6 and its
- // sprint table. Flying descent does not slow the processed horizontal axis.

## crates/sim/tests/it/liquid_contact_native.rs
- //! LiquidBlocksFetch senses the preceding pose before SwimTrigger changes it.

## crates/sim/tests/it/liquid_exit_native.rs
- //! Current-client liquid exit regressions. The native system probes the actual
- //! resolved pose box in f32, for every liquid travel mode (0x09004df0).

## crates/sim/tests/it/liquid_native.rs
- //! Focused regressions derived from current 1.26.50.26 canonical movement
- //! bodies and their matching PE data. These use a synthetic fully wet world;
- // RVA 0x0320fc20: sprint is ActorData bit 3; y uses the independent
- // water retention. RVA 0x0322d5d0 then applies water gravity outside swim.
- // Dispatch 0x09fdd550 excludes MobIsJumping; jump 0x0a5dc2e0 uses
- // the default liquid impulse. Swimming skips gravity (0x0322d5d0).
- // Current 0x09fd2140 selects the faster steering rate below its dive
- // WaterSinkInputSystem 0x0dc3db30 adds the descent input in water,
- // Current 0x09eeeb70 floors attach7 and copies its material's liquid byte.

## crates/sim/tests/it/surfaces.rs
- /// Current BedBlock restitution is 0.75, without a one-block velocity cap.

## crates/sim/tests/it/swim_jump_native.rs
- //! Native SwimAmountComponent/ActorHeadInWater guards in MobJumpSystem.
- // CurrentSwimAmountSystem 0x099e64c0 is registered before MobJumpSystem
- // 0x0a5dc2e0 and the current swim trigger. The first entry tick retains

## crates/sim/tests/it/terrain.rs
- /// MaxAutoStepComponent starts at 0.5625, below this obstacle's top.

## crates/ui/src/geometry.rs
- /// Physical pixels per GUI pixel: Bedrock's desktop rule
- /// (`GuiData::calculateOptimalGuiScaleIndex`), `min(width/376, height/250)` in

## crates/ui/src/hud.rs
- /// `ToastMessage` defaults and `ToastManager`.

## crates/ui/src/text/palette.rs
- //! Vanilla UIDefRepository::_applyGlobalColorFormat reads RGB triples and updates
- //! ColorFormat's shared table.

## docs/core-join-startup.md
- | Initialization | Send the local runtime ID once after loading state `0x10`, stable dimension state `0`, and `isInWorldAndNotShowingAnyMenuScreens` are satisfied. This is local readiness after the screen closes, rather than a direct PlayerSpawn response. |
- but the path that does so remains unresolved. Loading state 4 completes (0x10) only
- once loaded chunks reach the needed count or, after a deadline, every
- `_mChunksNeededForLoadOffsets` chunk is loaded, and a position check passes.
- State 0x200 completes directly unless the view's `bool` argument is set;
- `stopLoading` sets 0x10 directly, through callers not resolved here.

## docs/evidence/2026-10-01-zeqa-movement.md
- Jumping: input update carries held processed jump into MoveInput bit
- `0x10`; `fillInputPacket` maps it to wire bit 6. StartJumping:
- Jump initiation checks canJump, performs the jump, then sets action `0x100`;
- `setFromComponent` maps that action to wire bit 31.
- MovePlayer mode 2: `Player::handleMovePlayerPacket` sets action
- `0x40000000`; `setFromComponent` maps it to HandledTeleport bit 37.
- Current `LegacyClientNetworkHandler::handle(SetActorMotionPacket)`:
- 1. For a local replay actor and a **nonzero packet tick**, creates a position-delta replay object, calls `ReplayStateComponent::applyFrameCorrection` at that packet tick, and clears replay-state byte 1.
- 2. For **tick zero**, bypasses the replay object/history APIs and invokes the actor motion virtual immediately. It never substitutes a local receive tick.
- Immediate motion writes only the incoming vector to StateVector velocity offsets
- 0x18/0x20. It does not call other functions or write history, input flags, ground
- state or rotation.

## crates/client-ui/src/ui_runtime/presentation/forms/oreui/modal.rs
- Index bundle modal `Ug` (`Ug.Overlay`, `Ug.Header` over title bar `gm`, `Ug.Content`, `Ug.Text`,
  `Ug.Buttons`) and the modal menu `SV`/`CV`/`wV`.

## crates/client-ui/src/ui_runtime/presentation/forms/oreui/widgets.rs
- `button_face`: pressable `sf`/`bf`/`hf`; menus theme `--pressableElevated*` nine-slices.
- `menu_item`: dropdown item `bV` (classes `gV`) in `MV`; check icon `Fp`.

## crates/client-ui/src/ui_runtime/presentation/forms/oreui/theme.rs
- Role table: theme `pD` colour roles over the palette constants defined beside `Zc`.

## docs/evidence/desktop-video-settings.md
- `GuiData::GUI_SCALE_VALUES` is `[1, 2, 3, 4, 5, 6, 7, 8]`. Desktop minimum

## docs/home-promo-investigation.md
- | Images | Associate fetched images by message and image ID with a local Core::Path. |

## docs/oreui.md
- The client picks a tech stack per screen (`ScreenTechStackSelector::getTechStackForScreen`): a
- non-zero dev override wins (1 OreUI, 2 JSON-UI), then a preference option, then the screen's
- `isSelected() && isSupported()`. Treatment toggles are true only when the service's treatment list
- names them, so they default off. The local install's `routes.json` lists screen routes.

## docs/reference/actor-animation-clocks.md
- # Native actor animation clocks
- broader native actor-animation parity gate: ordinary actors still evaluate Molang
- The definition constructor compiles the default expression
- tick still does not represent every independent native animation-player instance.
- The native query getters use render interpolation fraction `alpha`:
- phase input but does not implement these native render-time getters. No teleport distance cutoff
- or teleport-specific native animation reset was established by this investigation.

## docs/reference/arrow-rendering.md
- # Native arrow entity rendering
- Native geometry parsing starts with the supplied
- uses the native face-size defaults for all entities, with explicit signed
- `minecraft:arrow` receives this exact native two-sided contract. The lookup
- `ExpressionQueries::getTargetYRotationBase`
- special-cases the arrow actor type: `target_y_rotation` is absolute actor
- rotation yaw, interpolated by the native frame alpha, not a mob's clamped
- `Actor::getInterpolatedBodyYaw` returns zero for
- the base actor; the data-driven renderer uses that value for
- Native actor event handling, case `0x27`, assigns
- zero and interpolates bone poses for display. Exact native per-render-frame
- version-matched native flight/embedded-arrow comparison.

## docs/reference/block-break-particles.md
- plus 0.5, and reading `BlockDestructionParticlesComponent`'s particle count.
- Its current getter returns **100** without a count override.
- `addTerrainParticleEffect` selects `minecraft:block_destruct`.
- `_addTerrainEffect` supplies the count, its cube-root intensity, velocity scalar 1 and radius 0.5 for these

## docs/reference/block-placement-prediction.md
- # Native block placement prediction
- fix. This is a live functional prediction witness, not native Bedrock/BDS parity acceptance.
- actor-overlap tolerances, repeat timing and complete native material/side-effect behavior.
- native ordering parity is not claimed. No full placement parity gate is closed. Changes

## docs/reference/camera-fov.md
- `LevelRendererPlayer::getFov` and `getFovWithoutGameplay`
- scale the configured angle by
- `min(normalized_viewport.y / normalized_viewport.x, 1)`. `ClientInstance::getNormalizedViewportSize` divides each viewport dimension by the corresponding
- full-screen dimension from `GuiData::ScreenSizeData`. The values are `(1, 1)`
- `CameraAPI::tryGetFOV` converts that angle to radians.
- `dragon::rendering::Camera::createPerspective` supplies the
- viewport width/height separately to `bx::mtxProjRh`. That
- projection places `cot(FOV / 2)` on the vertical axis and divides it by aspect

## docs/reference/carried-block-textures.md
- # Native carried block textures
- | Mip construction | Pass the parsed overlay from tile offset `0x30` to atlas mip construction as overlay argument 15. |

## docs/reference/crossbow-use.md
- # Native crossbow use state
- `getMaxUseDuration` is 25 ticks minus five ticks per Quick Charge
- level; loading does not change that duration query to zero. `use`
- checks the stack's cached charged projectile. An uncharged item starts use; a
- `releaseUsing` computes normalized draw power from duration minus
- `chargedItem` and retains a cached item stack. The crossbow's duration-depleted
- virtual implementation dispatches release with zero remaining
- After its item-complete gameplay event, `Player::completeUsingItem` checks
- `Level::isClientSide` and skips the transaction/depletion branch on the client.
- The sub-client id is a separate field.
- Only the server-side branch constructs an `ItemReleaseInventoryTransaction`:
- the transaction fills selected slot, player position and action `Use` (1),
- invokes `useTimeDepleted` and writes the mutated stack. That branch also records
- button release still sends a release transaction. `getAnimationFrame` selects frame 4 for loaded arrows and frame 5 for fireworks
- The current `setIcon` override registers five icon records from
- the `crossbow_pulling` atlas key, in variants zero through four. The
- `getIconInfo` override uses the ordinary standby icon for frame
- zero; nonzero animation frame N addresses registered record N minus one.
- HUD capture now routes every charged stack's icon through that mapping. Native
- loaded NBT applies to hotbar, inventory, offhand, storage, and cursor cells.
- and rendering evidence, not a version-matched native frame comparison.
- `Player.UseItem` call instead. Consequently the fixture cannot establish native

## docs/reference/crouch-camera.md
- `VanillaOffsetSystem` runs on both sides.
- | Version-selected crouch drop | Current version uses float bits `0x3eb33333` (`0.35`); the legacy branch’s `0.125` is not this client. |
- The client tick receives
- the actor data flags and optional `IsHorizontalPoseFlagComponent`; sneaking is
- flag bit 1. `UpdateHorizontalPoseSystem::update` admits
- gliding, swimming and crawling flags. The sleeping branch has a distinct native
- `0.2` target; that branch and riding/dynamic offset inputs are not implemented by
- `SneakTriggerActionSystem` consumes the input start/stop-sneaking bits and
- sets/clears actor flag bit 1. Current `SneakingSystem` tick adapter uses native base multiplier `0.300000012`; Swift Sneak
- adds `0.150000006` per level and caps at one. This audit does not
- claim the existing collision/Swift Sneak simulation is fully native-conformant.
- targets follow the native priority and tick blend. Frame alpha interpolates the
- behavior-contract tests, not a native rendered-frame
- on the macOS/Metal client. A controlled native standing/held Shift/release and

## docs/reference/dropped-items.md
- # Native dropped-item rendering
- Current `ItemActor` constructor sets the
- collision width and height to 0.25 and the native Y-origin offset to half the
- height. Current `MoveActorAbsoluteData` constructor copies the
- actor's StateVector position directly. `AddItemActorPacket`
- constructor copies that cached movement position; its
- `LegacyClientNetworkHandler` item spawn handler creates the actor
- at that position and retains it as the last received position. The item `clientInitialize` does not subtract the collision offset.
- `clientInitialize` retains this direct position setter.
- The rendering caller is equally important: `ActorRenderDispatcher` calls `Actor::getInterpolatedRidingPosition`.
- For an unmounted item that calls `getInterpolatedPosition` which
- interpolates current and previous StateVector XYZ directly. The dispatcher places the passed XYZ into `ActorRenderData` offsets 0x10, 0x14
- and 0x18; `ItemRenderer::render` translates by those values
- without subtracting the item offset. The alternate dispatch also builds that direct camera-relative origin. This establishes a native
- item origin above collision feet, rather than an extra invented mesh lift.
- retain `NetworkOffset` because native `MoveActorDeltaData::parseDeltas` merges
- positions into the previous absolute data.
- `ItemRenderer::getRenderYOffset` uses time
- and baseline 0.1. Ordinary block models additionally rise by 0.2. The Math
- table initializer samples `sinf(index / 10430.3779296875)`;
- lookup truncates `angle * 10430.3779296875` and masks to 16 bits.
- On the first render only, an actor whose current native origin is less than
- zero over 0.4 seconds. `easeInCubic` establishes this as
- spin. The captured camera yaw contributes using the native degree-conversion
- `ItemRenderer::render` uses one, two, three and four copies for stack counts
- below 2, below 6, below 21 and at least 21 respectively. The renderer
- constructor owns its random XYZ copy table, shared across actors;
- the first copy stays centered. `_renderItemGroup` translates
- later copies by that table times `0.2 / groupScale` inside the spinning and
- The ordinary flat-item route uses group scale 0.3 and shared
- `ItemInHandRenderer` with the dropped-item flag, not a hand grip.
- and rotations to the raster frame. Combined with `TextureTessellator`, this maps a raster point `[column, depth, row]` to local
- The native legacy tessellation route queries the item's animation frame before
- its icon. Loaded crossbow sprites share the same `getAnimationFrame` /
- `getIconInfo` selector as HUD and inventory icons, using canonical charged
- brightness, native sine lookup, first-render camera capture/easing, rotated
- frame checks, not a version-matched native gallery or pickup-trajectory parity.
- transforms require their native model routes, not a flat GUI thumbnail. This
- Multi-layer/enchantment material behavior, native actor lighting/shader parity
- but the local generator is ours, not the native generator; platform sine
- native pickup-trajectory implementation.

## docs/reference/effective-movement-speed.md
- `Player::getSpeed` reads `minecraft:movement` from AttributeInstance current. `BaseAttributeMap::updateAttribute`
- replaces the previous modifier set with the packet's modifiers and
- `LocalPlayer::setSprinting` and
- `SprintTriggerSystem::setSprinting` return on an unchanged actor
- flag. `Mob::setSprinting` adds or removes only its identified sprint

## docs/reference/emote-wheel.md
- ## Identified references
- Lens's 26.30 reference and the current reconstructed Windows client identify:
- at `0x10096bc20` and full layout at `0x10096e1f0`, action `0x34`, keyboard B.
- The current Windows mapper at RVA `0x7395f10` names that action `key.emote`.
- `persona_common.emote_wheel_panel`: four cardinal slots. The controller's
- contextual global bindings query each originating control's `#index`.
- action `0x34` to native button 7, identified as D-pad Left by the sprite/name
- initializer at `0x102e15a30`. The controller mapper at `0x10097ef10` binds it
- to `button.emote`. Supplemental binding rows are appended to preserve saved IDs.
- `PersonaAppearance::setEmote` at `0x103c3b260`: bounds slots and swaps an
- already equipped piece into the requested slot.
- rectangle side; the inner boundary is excluded and the outer is included.
- Visibility at `0x1024e9f30` selects one state, and construction at `0x1024e9af0`
- starts with no hovered slice.
- 1.26.50.26 RVA `0x4f382c0`: animation controller playback and emoting status.
- This function does not directly change the camera perspective.
- `ClientInputUpdateSystem::updateStopEmotingRequest` at `0x1022bc6a0`:
- nonzero horizontal movement requests cancellation.
- doll visible with the existing hold timer.

## docs/reference/farmland-rendering.md
- incorrect generic diagonal cross. Wheat now uses the native four-row model,
- ## Current Bedrock source evidence
- The local MCSRC reconstruction at revision
- `da728f0ce4d7a5ae0be443b8abe03119858d923e` was inspected for the current
- `1.26.50.26` client. Recovered names aid navigation; canonical function records
- and the matching executable establish identity. The executable SHA-256 is
- `7d6cf9b2e4b01fce5d6283cc3deb65b877995a8fd1e146f967d8ac743369d628`.
- References below are RVAs under `current/1.26.50.26/`; source and executable
- payloads remain outside this repository.
- `src/__recovered/BlockTypeRegistry.h`, calls constructor `08715fd0`.
- visual shape `(0,0,0)..(1,15/16,1)`. Initializer `05efb820` sets those bounds;
- its raw reconstruction was checked against the matching PE disassembly and
- data, including the packed maximum Y/Z words.
- whether moisture is below one. The pinned pack binds dry to index one and
- wet to index zero, so moisture zero is dry and all other states are wet.
- `src/__recovered/BlockTessellator.cpp`, calls vtable slot `+0x330`.
- The matching FarmBlock vtable points that slot at `08716d90`, confirming
- that the selector is used during tessellation.
- `src/__unmapped/06.cpp`, builds graphics from pack names and registered
- block types; it does not establish the legacy sequential-ID formula.
- The older native gallery and UV limitations remain documented in
- `docs/evidence/phase-2-farmland-native-reference.md`. The current source confirms
- height and top selection; no new claim about calibrated source rows is made.
- The older named `BlockTessellator::tessellateRowInWorld` and
- `tessellateRowTexture` bodies identify the row path. The current canonical bodies
- and matching PE verify the coordinates and UV order:
- adds the float at VA `15014e720` to block Y; its verified value is `-0.0625`.
- Disassembly `06a21ff5..06a2201b` confirms the offset is passed to the helper.
- side for each. PE constants at VAs `14fec3380`, `1500eb850`, `14feff2e0`,
- `14ff1b150`, and `14fea4060` are respectively `0.5`, `-0.25`, `-0.5`,
- `0.25`, and `1`. These yield rows at X/Z 1/4 and 3/4, spanning 0..1.
- The resulting Y range is -1/16..15/16 relative to the wheat block.
- their world direction. Cinnabar represents the native reverse sides with its
- near-version witness to the analyzed preview, rather than an exact-version gate.

## docs/reference/first-person-offhand.md
- # Native first-person offhand placement
- `ItemInHandRenderer::renderOffhandItem` selects the cached offhand
- stack at renderer offset `0xd0`, builds its render key using animation frame `-1`, pushes
- the camera matrix, and draws the cached item. It does not
- re-enter the ordinary main-hand `renderItem` transform after applying its camera pose.
- Angles below are degrees. These are native tessellator-frame matrices, not an instruction
- For `Item::isHandEquipped() == false`:
- For `Item::isHandEquipped() == true`, unless the legacy Shield-blocking special case wins:
- | Native pixel-to-model scale | `0.0625` |
- The table shows equivalent angles; the native rotation constants use radians. The hand-equipped depth is `1.53125`,
- `_rebuildItem` stores `16/max(width,height)` in the cache. The flat offhand branch
- The flat branch therefore normalizes the native pixel geometry to one model unit on
- `TextureTessellator::tessellate` emits positive column
- normalized native point is `(-held.x, -held.z, height/max - held.y)`. Thus the basis
- display transform for presentation type `2`. The native default presentation array
- has zero translation/pivots, Y rotation `-135` degrees and scale
- `0.4`. Constructor negates Y/Z for type 2, yielding
- Current `renderFirstPerson` writes
- `context.player_offhand_arm_height` separately from `variable.player_arm_height`.
- `previous_offhand_height + (current_offhand_height-previous_offhand_height)*frame_alpha`,
- using renderer offsets `0x18c` and `0x188`. Main-hand heights use `0x184` and `0x180`.
- Native tick snapshots both hands independently, advances each toward
- and stack-specific native instant-update/equivalence predicates (the current retained
- equipment feed supplies identifiers, not the full native cached ItemStack comparison).
- matched native gallery or acceptance of the incomplete routes above.

## docs/reference/fish-rendering.md
- Native FishAnimationSystem copies the current phase to previous, then advances
- current by `1 + 0.1 * length(StateVector.velocity)` each tick. The native variable
- updater publishes `variable.AnimationAmount` and `variable.AnimationAmountPrev`

## docs/reference/flight-control-corrections.md
- The analyzed reference is the canonical reconstruction of client `1.26.50.26`
- in the private MCSRC workspace. The repository target game version is selected
- by `assets/bedrock-target.json`; the available client reconstruction is a preview
- build in that version family. Source bodies remain private and are not copied
- into the repository.
- ## Identified native behavior
- position, motion and AABB. It does not clear movement abilities or input mode.
- teleport route before advancing the live actor. Its directly reconstructed
- spatial operations do not reset the flight trigger or movement abilities.
- its full structural correspondence with the named reference counterpart
- `0x059aaf10`. The local flight state lives in the player input request and is
- toggled by input; the double-tap countdown is seven simulation ticks.
- named reference `0x059ab030`. Flight start/stop requests write boolean ability
- 9 and clear the flight countdown and fall distance.
- internal flight action bits 34/35 into wire input ordinals 42/43.
- mode. Its `PosDelta` uses end-of-tick motion, matching the native send path;
- ## Identified flight travel
- The named older `VerticalFlySpeedControlSystem::doFlySpeedControlSystem`
- counterpart identifies current RVA `0x02c452b0`. The current body and matching
- executable float data establish these control operations before movement:
- float `0.01`. Creative idle flight multiplies existing vertical motion by
- `0.375` only while neither vertical control is held.
- `HorizontalFlySpeedControl`, current RVA `0x03235360`, reads float ability 6 and
- the matching executable's `[2, 1]` sprint multiplier table. The keyboard
- vertical controls read float ability 7 in the vertical control body. Both
- custom speeds and explicit zero are retained.
- The current ability default constructor at RVA `0x001dd510` initializes
- protocol ability 13 (`FlySpeed`) to native float `0.05`, and ability 19
- (`VerticalFlySpeed`) to native float `1.0`. These match the fallback values
- `FlyDrag`, current RVA `0x03217940`, reads friction coefficient
- `0.3999999761581421`. The native float subtraction from one produces vertical
- retention `0.6000000238418579`, independently of horizontal hover friction.
- Horizontal drag, current RVA `0x03203a50`, multiplies the horizontal modifier
- by ordinary air friction and clears each horizontal lane at float epsilon.
- The flying travel wrapper at current RVA `0x099cce30` calls shared horizontal
- movement at `0x099cc8b0`. That movement reads ground contact before collision
- and selects the friction of the block at starting feet minus native float
- `0.1`. The current `MobTravelComponent` constructor at `0x036ac750` enables
- this ground-friction probe and leaves vertical generic friction disabled.
- The flying friction wrapper at `0x0321f020` therefore retains horizontal
- motion by `ground friction * hover modifier * air friction`, while separate
- flight drag retains vertical motion. Grounded flight now samples that exact
- support coordinate with the ordinary query bound and world-identity checks.
- Its regressions include hover, takeoff, landing and fractional support height;
- takeoff and landing use the starting contact state for that tick's friction.
- The current send-input mapper at RVA `0x070fcfd0` maps raw input bits 17/18 to
- wire `Ascend`/`Descend`, and raw bits 2/3 to wire
- `WantDownSlow`/`WantUpSlow`. The current local input updater at RVA `0x07108cc0`
- combines keyboard jump or raw ascend into processed up with mask `0x20080`,
- and keyboard sneak or raw descend into processed down with mask `0x40001`.
- Those processed controls are then sent as `WantUp`/`WantDown`, wire ordinals
- 16/17. The extra raw-input acceleration lanes in the native vertical
- controller correspond to the distinct slow controls, which Cinnabar does not
- emit.
- The current server input handler at RVA `0x0998fe80` reconstructs processed
- up/down directly from `WantUp`/`WantDown`. It reconstructs raw
- `Ascend`/`Descend` and `JumpDown`/`SneakDown` separately; those raw flags do not
- populate processed up/down. The handler is identified through the named
- `ServerMoveInputHandlerSystemUtils` adapter at RVA `0x099abbb0`, whose entity
- dispatch at `0x09990430` selects this body. The following input-lock operation
- at `0x00480c40` only clears restricted raw inputs.
- processed controls with held jump and sneak. The regression reconstructs the
- native server control mask from the outbound flags for up, down, both and
- released controls. The protocol regression verifies their named wire rows.
- double-tap detector remains an approximation of the native tick countdown;

## docs/reference/fox-rendering.md
- ## Current client evidence
- MCSRC's verified current export is preview 1.26.50.26. Its matching executable
- SHA-256 is `7d6cf9b2e4b01fce5d6283cc3deb65b877995a8fd1e146f967d8ac743369d628`.
- The named 26.30 reference supplies navigation; the contracts below were checked
- against current canonical bodies or the matching executable.
- reconstructed body is unavailable, so its bind-field read was verified by
- disassembly of the matching executable. VA `146aea6c5` loads
- `bind_pose_rotation`; `146aea71e` calls hierarchy getter `00b95650`.
- `146aea7d8` requires three array entries. `146aea841` through `146aea84b`
- store the converted bind radians separately at node offsets `0x38..0x40`.
- nodes when a field is absent from a replacement. A missing bind in a modern
- replacement therefore retains the base model's bind.
- passes bind rotation at `+0x38` separately to `01e62d50`. The latter combines
- cube and bind Euler angles and rotates the cube pivot about the part pivot.
- Children retain their authored pivots and animation frames.
- ## Native resource witness
- native side-by-side acceptance were not performed.

## docs/reference/game-mode-updates.md
- Current `UpdatePlayerGameType::getId` returns the same packet
- ID as our generated `McpePacketName::UpdatePlayerGameTypePacket`; we use the enum,
- not a copied numeric ID. The generated packet preserves a game type, signed actor
- `ClientNetworkHandler::handle(UpdatePlayerGameTypePacket)` matches the packet target against player-list **unique IDs**. A
- `Player::getPlayerGameType` resolves raw game type
- `5` through the level's default game type. `Player::setPlayerGameType` retains that raw default binding while using the effective mode for
- its mode-change work. Cinnabar reuses its existing player/default-mode reducer:
- functional regression, not historical replay or full native visual parity.
- `GameMode::startDestroyBlock` and
- `continueDestroyBlock` select the creative
- destruction route through `Actor::isCreative`.
- That predicate reads only the game-type component: Creative, or world-default
- `Player::getDestroyProgress` passes **Flying**
- into the destroy context. `PlayerDestroy::getDestroyProgress` and `getDestroySpeed`
- apply the speed, hardness,
- harvest and movement penalties; neither selects instant destruction from Instabuild.
- Current ability serialization independently distinguishes Flying
- at offset `+0x6c` from Instabuild at `+0x84`.
- Other ability grants and the passive evidence owner are unchanged. Native mode-layer
- `ClientPlayerRewindListener::_onUpdatePlayerGameTypePacketReceived` applies a tick-zero packet immediately. With a nonzero tick and
- an eligible replay timeline, it instead inserts `GameTypeReplay` at that tick and

## docs/reference/held-attachables.md
- # Native animated held items
- `ItemInHandRenderer` skips its legacy icon placement
- when an active attachable is present. First-person rendering evaluates the owner’s
- `setupAttachableNoChecks` composes the parent's matrix before the
- item's channels. Treating this as the third-person sprite grip was the bow bug.
- The owner-skeleton camera root also retains the native post-scale 1/128-model-unit
- Native `TextureTessellator` admits pixels with alpha at least 2.
- `compileQuads` composes position minus the authored bone pivot, Z/Y/X Euler
- rotation, negative local pivot, and texture/model scale, in that order. The loader performs the bone-pivot subtraction. Native model Y is converted
- Its animation frame comes from `RangedWeaponItem`: for elapsed
- native frame comparison or a complete visual parity gate.
- The packet handler calls the `ItemRegistryRef` wrapper, which invokes
- `ItemRegistry::matchServerItemIds`.
- Its initialization state at offset `0x331` gates execution: state 3 returns
- without changing the registry, and successful initialization finishes in state 3.
- Repeated packets are not runtime registry replacements.

## docs/reference/hud-paper-doll.md
- | Model origin | Retain the 24-pixel ModelPart origin scaled by the player model scale; swimming adds a 0.8 vertical adjustment. |
- native 24-pixel ModelPart origin, scaled by the player model scale; its control

## docs/reference/inventory-block-restacking.md
- Native occupied-stack compatibility is not a requirement for zero aux or zero
- block identity:
- wildcard), user data, restriction hashes and an additional field at `+0x70`.
- A present left-hand block pointer at `+0x18` must match the right-hand block
- pointer; merely being present is not a rejection. Its charged-item path has
- further checks and is outside this fix.
- other stack. It compares aux when the item's variant flag requires it, then
- user data, the restriction hashes and the additional `+0x70` field. This
- function does not itself compare the block pointer at `+0x18`.
- data and an empty compound tag can compare equal. Native support is broader
- than admitting only empty serialized data.
- The restriction hashes are CanDestroyHash at `+0x68` and CanPlaceOnHash at `+0x48`.
- The extra `+0x70` comparison is retained as an
- identified field, not assigned an unverified semantic name. For ordinary
- occupied stacks, count and server/sparse stack-network IDs are not semantic
- item equality keys; they remain necessary for quantities and request authority.
- aux checking enabled (`sameItemAndAux`). Recipe selection separately calls
- full-stack `matchesItem` when merging items into an occupied grid slot.
- non-null block pointer, including a wildcard block-type descriptor, before
- merge prediction path, even when native structural comparison would accept it.
- The scoped equality checks also do not reproduce native variant-flag exceptions,

## docs/reference/inventory-gui-geometry.md
- | Shield GUI transform | Shield GUI transform and ModelPart draw |
- rotation, so Y acts first on authored points. The native rotation angles have bits
- `0x406a927f` and `0x3f490fdb`; the scale/vertical offset have bits `0x41200000` and
- `0x4147ae14`. The displayed icon frame is 16 design pixels, not 16 physical pixels.
- The ordinary cube GUI path emits only Up, South and West, in that order. Their native
- Static ModelPart pivot-relative coordinates combine into `(x,24-y,z)` before the GUI matrix.
- See [shield inventory rules](shield-inventory-icon.md) for the ModelPart witnesses.
- builders therefore disable depth test/write and preserve native authored draw order.
- exact native model-material texture/color transfer-function formats and a controlled vanilla frame comparison remain
- shield NBT layers, native glint and custom rotated/inherited/animated shield ModelParts

## docs/reference/inventory-hover-tooltip.md
- The constructor accepts only an integer JSON value for `hover_text_max_width`.
- number leaves it unrestricted, as in the native constructor.

## docs/reference/inventory-normal-transactions.md
- cells, with the native cursor projection. Unknown or unreviewed sources/slots,
- The native UI output-50 deferred `InventoryTransactionManager` path, arbitrary

## docs/reference/inventory-recipe-admission.md
- those conditions, the state pointer remains null and candidate recipes still

## docs/reference/inventory-reopen.md
- `onContainerScreenClose` removes the pending screen when the retained screen queue

## docs/reference/inventory-sparse-prediction.md
- # Native sparse inventory prediction
- | `SparseContainer::getItem` | Return the absolute sparse item when the cell is predicted; otherwise return its backing item. |
- | `SparseContainer::setItem` | Save the new sparse item and invoke the set listener. |
- | `SparseContainerSetListenerClient::postSetItem` | Stamp every changed item with the current typed request id, including an emptied item, and register its container with the request. |
- | `SparseContainerClient::_networkUpdateItem` | Update the backing container without rebasing or subtracting an active prediction. |
- | `ItemStackRequestActionHandler::_validateRequestSlot` | Resolve odd-negative request references through request-id, container-runtime-id, and requested-slot assignments. A request id is not a globally unique item identity. |
- | `ItemStackNetManagerClient::handleItemStackResponse` | Find the issued request across retained screens; skip unknown ids. Process each answer immediately. |
- | `SparseContainerClient::tryPushSlotPrediction` | Requested slot locates the sparse item; actual slot receives the correction. Validate amount/net-id pairing. A later owner selects the historic path. A missing sparse cell is skipped. |
- | `SparseContainerClient::_pushHistoricPredictionItem` | Correct backing using the request's historic item without removing the newer active prediction. |
- | `SparseContainerClient::clearAllPredictions` | Remove remaining active cells whose stamp is the answered request, not older or later owners. |
- | `ItemStackNetManagerClient::_clearPredictiveContainerRequest` | Remove the answered historic snapshot and clear that request's active sparse cells. |
- | `ItemStackNetManagerBase::onContainerScreenClose` | Retire the oldest retained screen after close acknowledgement. Its late replies no longer own a retained screen. |
- the retained pre-empty item, matching native zeroed-out-item handling. Count/id
- proxy delta contracts to the identified native contracts.
- policies remain Cinnabar safety policies, not a claim that every native inventory

## docs/reference/item-particle-lighting.md
- ## Current-client lighting contracts
- `ItemInHandRenderer::renderItem` is the ordinary dropped-item
- tessellation consumer identified in [dropped-items.md](dropped-items.md).
- Its actor lighting setup supplies `(sky, block) / 16` to
- `LightTexture::getColorForUV`. The latter samples normalized byte
- Modern pack particles use the same RGB lookup. Current
- `ParticleSystemEngine::tick` constructs its
- 16-by-16 gameplay-light cache by calling `LightTexture::getColorForUV` for each pair of nibbles
- multiplied by `0.0625`.
- `ParticleEmitterActual::getGameplayLightForParticle` retrieves
- the four-channel cached entry for the particle's brightness pair.
- `getBrightnessPairForParticle` floors world particle coordinates,
- The ordinary native material color contract composes normalized gamma RGB;
- shaders undo the atlas decode, multiply native tint and RGB lighting, then
- Dropped items now use the byte-quantized `/16` lookup and native color
- native byte lookup in darkness, daylight, sky light at night, torchlight,
- version-matched native scene or performance parity gate.
- material handling. Native item AABB brightness sampling is not replaced by

## docs/reference/jsonui-review-fixes.md
- indices, share instance keys between binding and layout, seed native creation
- | Perspective | Register the perspective option’s callback and apply `_perspectiveOptionChanged` only for a perspective change. |
- | Font measurement | Obtain the current FontHandle for both measured text and drawing. `ui/ui_template_dialogs.json:9` defines the standard title label; the accepted open-font deviation still requires shared installed metrics. |
- These rules do not establish a native frame-time budget for the captured shop.

## docs/reference/liquid-currents.md
- The reference is the canonical reconstructed 1.26.50.26 client and its matching
- PE, SHA-256 `7d6cf9b2e4b01fce5d6283cc3deb65b877995a8fd1e146f967d8ac743369d628`.
- The older named 26.30 reconstruction supplies identities; current bodies and PE
- data supply the behavior below. Cinnabar's implementation is independently
- written from this evidence.
- Current `LiquidPhysicsSystem::_liquidBlockFetch` is **RVA 0x0a5d5c40**. Its
- adapter is **0x0a5e8d50**. Registration **0x072c5ec0** installs the flow-policy
- writer, then this block fetch, then lava, water and head sensing. Only afterward
- does it register player triggers and pose updates through **0x072bb110** and
- **0x072b6020**. Consequently current acts on retained velocity before jumping,
- lava uses `0.1` and `0.4`. Inverted axes clamp to the original center. Material
- gather **0x0a5dd330** traverses Y, then Z, then X, with a floored lower bound and
- exclusive `floor(f32(upper + 1))` range end. It gathers resolved-liquid cells
- using IConstBlockSource slot `+0x28`: the extra non-air block takes precedence,
- with primary fallback when extra is air. This is the getter **0x0319e4f0** in
- vtable `0x150161770`. When both probes contact their liquid, the fetch selects
- lava for current force.
- Local flow-policy writer **0x0a5e6f90** admits ordinary current when the prior
- flying ability is clear. `FlyTriggerSystem` intent **0x06dcdf80**, registered
- after swimming intent inside **0x072b6020**, and its action **0x06dce090** run
- later. The input therefore retains the preceding flow allowance for live ticks
- and replay, independently of the newly selected flight mode.
- with matching material and nonzero raw depth admits current. The entry's native
- boundary bits are `0x04`, `0x20`, `0x08` and `0x10`, respectively. Falling depths
- still admit current; collapsing them to an effective depth zero too early would
- incorrectly disable the force.
- Once admitted, every collected cell contributes its vector from
- **0x0395d2f0**, including source-depth-zero cells. That helper already normalizes
- each cell's result. The fetch sums these vectors in its native Y/Z/X cell order,
- normalizes the aggregate again, multiplies by the selected material's impulse,
- and adds it to the retained velocity. The force does not grow with the number
- of touching cells and is not an average of separately scaled impulses.
- Both normalization steps compute native float squared length as
- The matching PE confirms this grouping at VA `0x14395d72d` and `0x14395d793`
- inside the cell helper and `0x14a5d6826` inside the aggregate; the reconstructed
- expression's operand order differs. Matching PE values are:
- | VA | Native float | Meaning |
- | --- | ---: | --- |
- | `0x15005b1dc` | `0.0000999999975` | Normalization threshold |
- | `0x1500d3a40` | `0.0140000004` | Water impulse |
- | `0x1500d3a3c` | `0.00350000011` | Lava impulse |
- | `0x14fea4060` | `1` | Exclusive gather end addition |
- the raw state. A legacy world or a per-cell vector whose native obstruction facts
- are unavailable also returns no current authority, preserving the incomplete
- boundary instead of inventing a vector from collision geometry. Mounted ownership and
- non-player/item-actor liquid probes remain outside this player implementation.
- ## Per-cell direction and native obstruction facts
- `world/flow.rs` implements current **0x0395d2f0**, identified by the named
- 26.30 `LiquidBlockBase::_getFlow` at **0x0ab193c0**. Production registrations
- retain BREG `ModelStateField::LiquidDepth` without collapsing values 8–15.
- The helper uses raw depth for the falling branch and effective depth zero for
- those values when comparing neighboring levels.
- The matching PE's planar facing bytes at **0x15013e0c7** are `2, 5, 3, 4`
- (Z-, X+, Z+, X-). Its masks at **0x1502a38a8** are `1 << facing` and the
- opposite-facing table at **0x1500e01e0** swaps each adjacent facing pair.
- For a matching neighboring liquid, both primary blocks must admit the
- corresponding face. The contribution is `(neighbor depth - current depth)`
- times that direction. A rejected face follows the same fallback as an
- unmatched liquid: when the primary neighbor's material does not block motion,
- a matching liquid beneath it contributes
- `(below-neighbor depth - current depth + 8)` times that direction.
- The getters are intentionally distinct. Matching PE reads neighboring liquid
- depth through slot **+0x28** at **0x14395d42a**, primary material's motion byte
- through **+0x10** at **0x14395d451**, and below-neighbor depth through
- **+0x28** at **0x14395d48c**. Directional face reads use primary **+0x10**
- at **0x14395d535** and **0x14395d58e**, followed by BlockType's virtual
- **+0x88** at **0x14395d568** and **0x14395d5bd**. The falling-wall checks
- also read primary **+0x10**, at **0x14395d6cd** and **0x14395d703**.
- normalizes again. The matching constant is **0x150068e7c**. Each arithmetic
- step remains float; widening happens after the normalized vector is complete.
- Native material setup **0x0379bed0** establishes byte **+3** (`blocksMotion`)
- and byte **+5** (`isSolid`). Types 0, 5 and 6 have both clear; types 1, 13 and
- 23 have both set. The bindings below come from constructors, not PREG
- passability, render opacity, collision boxes, or BREG face coverage.
- | Binding | Current source association | Material type |
- | --- | --- | ---: |
- | Air | Native air material setup | 0 |
- | Water / flowing water | Liquid material setup; raw BREG LiquidDepth | 5 |
- | Lava / flowing lava | Liquid material setup; raw BREG LiquidDepth | 6 |
- | Dirt | DirtBlock constructor **0x0a7b9820** | 1 |
- | Grass | GrassBlockBase constructor **0x0712ab00** | 1 |
- | Stone | registerBlock<StoneBlock> **0x0dfb6010** → **0x0a5b7aa0** | 23 |
- | Sand | registerBlock<SandBlock> **0x0dfb97a0** → **0x08efbad0** | 23 |
- | Gravel | Constructor **0x0712be60**, vtable **0x1502a5290** | 23 |
- | Ice / packed ice | registerBlock<IceBlock> **0x0dfc49c0** → **0x071305c0** | 13 / 23 |
- Gravel's vtable resolves its dust producer to **0x0712c030**, which selects
- the matching PE's gravel particle identifier at **0x15064f6f4**. This binds
- the otherwise unnamed constructor to GravelBlock. The listed class vtables'
- directional slot +0x88 resolves to **0x00085800**, which returns true.
- LiquidBlock's current constructor is **0x0395a220**; its vtable
- **0x1501905d0** and LiquidBlockBase's **0x150190a00** share that default.
- The face cache at Block **+0xb9** is the packed
- BlockLiquidDetectionComponent mask, separate from render face coverage.
- Named 26.30 `BlockComponentDirectData::_finalizeInit` **0x0b15f8e0** places
- that component in direct data at Block **+0xb8**. The current component
- initializer **0x0ab6b560** initializes the packed value to `0x20000`, whose
- mask byte is zero, matching the ordinary default cache. Special directional
- overrides and liquid detection rules require their own source-established
- registrations.
- below-neighbor liquid is absent: both native fallback outcomes are zero in
- that case. A required unknown directional or material fact returns no flow
- authority. Water-like blocks without proven raw LiquidDepth, including bubble
- columns, also remain unavailable. No fluid-height inverse supplies those
- facts. This keeps waterlogged special shapes, remaining native materials,
- bubble-column forces and non-player probes explicitly outside the completed
- ordinary current behavior.

## docs/reference/liquid-movement.md
- The movement target comes from `assets/bedrock-target.json`. This investigation
- uses the canonical reconstructed **1.26.50.26 preview client**, Lens artifact 6,
- and the matching executable SHA-256
- `7d6cf9b2e4b01fce5d6283cc3deb65b877995a8fd1e146f967d8ac743369d628`.
- The named 26.30 reconstruction identifies call meanings; every formula below
- was checked in the current canonical body and matching PE data. The
- reconstruction is derived evidence, not Mojang's original source. Implementation
- and tests are independently written.
- Current liquid physics **RVA 0x0a5d5c40** forms the water-contact AABB by shrinking
- the horizontal axes by `0.001` and the vertical axis by `0.401`. Lava uses `0.1`
- and `0.4`. A shrink that would invert an axis clamps it to the original center.
- The material-cell scan **0x0a5dd330** floors the lower bound and includes cells
- whose integer coordinate is at most the upper bound. Ordinary contact does not
- compare the actor with the rendered liquid surface. Cinnabar's existing
- Liquid contact precedes this tick's pose change. Registration **0x072c5ec0**
- installs `LiquidBlocksFetch` (current source `07.cpp`, line 470993; PE vtable
- `0x150376d90`, tick **0x0a5e8d50**, callback **0x0a5d5c40**), then lava, water
- and head sensing, before calling the player swim-trigger and pose registration.
- The callback reads the current AABBShape and does not sweep it by StateVector
- velocity. The resulting WasInWater component then supplies jumping and travel
- selection after the new collision height has been applied.
- sensing. PE **0x0a5dd330** calls IConstBlockSource slot `+0x28`; matching vtable
- `0x150161770` selects **0x0319e4f0**, which reads the extra block through
- `+0x20` and falls back to the primary block through `+0x10` when its type is air.
- Initialization **0x02e431b0** identifies the comparison value `0x1551c6d40` as
- `minecraft:air`. Existing all-layer liquid flags cover ordinary primary liquid
- and waterlogged secondary water. Exact priority for a non-air secondary block
- that conflicts with a primary liquid remains a boundary of this model.
- ## Proven water forces
- Water travel acceleration **0x0dc3eeb0** blends the water movement attribute
- toward the effective ground movement attribute by capped Depth Strider level
- divided by its maximum level. The effective level is halved while airborne.
- Cinnabar's existing `water_travel_speed` already implements the ordinary-player
- branch of this rule.
- Water drag **0x0320fc20**, identified through its current adapter's
- `MobMovementDrag::tickApplyWaterDrag` signature, acts on retained velocity after
- movement. Its baseline horizontal retention is `0.9` while the actor's sprint
- flag is set, independently of swimming pose. Otherwise it reads optional
- WaterMovement or the default `0.8`. Depth Strider blends horizontal retention
- toward `0.546000063`. Vertical retention is always the independent default
- water value `0.800000012`; enchantment does not change it.
- The current player's water-gravity body **0x0322d5d0** installs `-0.005`
- (`0xbba3d70a`) only when actor flag 57, swimming, is clear. A swimmer has no
- ordinary water gravity. Levitation uses a separate native travel path.
- The ordinary held liquid-jump branch in **0x0a5dc2e0** adds `0.0399999991`
- to vertical velocity before collision resolution. Its default comes from PE
- VA `0x1500b5374`. The branch retains native float addition.
- WaterSinkInputSystem's current adapter **0x0dc47ac0** calls ticking wrapper
- **0x0dc3db60**, whose callback is **0x0dc3db30**. With WasInWater present,
- held sneak/descent input (MoveInput byte 0 bit 2 or byte `0x60` bit 3) adds
- `-0.0399999991` from PE VA `0x150106adc` to vertical velocity. Flying ability
- suppresses this water sink force. Ordinary water sneak therefore changes vertical
- motion before movement, in addition to reducing horizontal input.
- Swimming steering **0x09fd2140** reads the negative-pitch sine table and moves
- vertical velocity toward that target using rate `0.0599999987`, or
- `0.0850000009` when the target is below `-0.200000003`. Its current dispatcher
- **0x09fdd550** excludes `MobIsJumpingFlagComponent`; held jump therefore bypasses
- pitch steering and enters the jump system. All steering products and addition
- are native float operations. Taking the sine table at negative pitch is distinct
- from negating the positive-pitch lookup at non-cardinal angles.
- The current source chain establishes the point and material semantics:
- offset to **0x0284dd20**. Bounding-box size update **0x02c33d10**, and its
- single-entity counterpart **0x02c39d40**, copy that offset into OffsetsComponent
- bytes `0x38..0x40`.
- calculation **0x037517b0** reads precisely those three floats. For an ordinary
- unmounted player, they are zero and the point is the native player anchor
- minus VanillaOffset. The caller selects interpolation zero, retaining the
- previous pose offset rather than the camera's render-frame interpolation.
- byte 2 into PlayerInputRequest byte `0x21`. Material setup **0x0379bed0** sets
- that byte for water and lava and clears it for air and ordinary solids.
- Matching executable disassembly confirms the virtual default-block slot at
- `0x10`. The current BlockSource vtable at image VA `0x150161770` selects
- **0x0319e020**, which reads the default chunk storage through **0x037af620**.
- It does not scan secondary liquid storage or compare fluid height.
- unspecified for legacy traces. The native previous-offset selection is proven;
- client comparison. Native start/continue/stop and current registration order
- are recorded in [swimming-trigger.md](swimming-trigger.md).
- Relevant current PE float words were read through PE section mapping, with
- image base `0x140000000`:
- | Image VA | Value | Consumer |
- | --- | --- | --- |
- | `0x14ff9c370` | `0.899999976` | Sprint water horizontal drag |
- | `0x15005ea28` | `0.800000012` | Water vertical/default horizontal drag |
- | `0x150167298` | `0.546000063` | Depth Strider horizontal drag target |
- | `0x150361800` | `0.0599999987` | Ordinary swim steering rate |
- | `0x150361804` | `0.0850000009` | Diving swim steering rate |
- | `0x14ffab668` | `-0.200000003` | Dive steering threshold |
- | `0x1500b5374` | `0.0399999991` | Default held liquid jump |
- | `0x150106adc` | `-0.0399999991` | Held water descent |
- derived from these bodies and PE values in a synthetic submerged world. They
- cover sprint drag without a swimming pose, independent vertical retention with
- Depth Strider, jump ascent independent of look pitch, and dive steering followed
- by water drag without gravity. They are numeric source-derived regressions,
- not recorded vanilla trajectories or server-acceptance evidence.
- The surface regressions also cover the exact floored attach-cell boundary,
- material contact above a shallow rendered liquid surface, primary air with
- secondary water, and downward steering with a dry attach point.
- CurrentSwimAmountSystem **0x099e64c0** saves the preceding amount and adds
- native `0.100000001` while actor flag 57 (swimming) or flag 114 (crawling)
- is set, capped at one. Otherwise it adds `-0.100000001`, floored at zero.
- These constants are PE VAs `0x14ffab644` and `0x14ffab670`.
- The writer runs before this tick's swim trigger and MobJumpSystem. This follows
- the actual registration chain, rather than inferring an order from metadata:
- **0x072ce0d0** registers CurrentSwimAmount, then invokes **0x072c5ec0**;
- that invokes **0x072bb110**, which invokes **0x072b6020** to register the
- swim trigger before registering MobJump. Collection **0x028e64f0** appends
- each identity to its category's vector, and **0x028e8f00** traverses it forward.
- Current `07.cpp` call sites are lines 476769, 481041, 473675, 462530,
- 459956 and 463035. The source ordering matters at both transition edges.
- MobJumpSystem **0x0a5dc2e0** returns before all ordinary liquid/ground jump
- paths when `0 < SwimAmount < 1`, or when swimming is set but the
- ActorHeadInWater component is absent. If WasInWater is present, this return
- also zeroes retained vertical velocity. Dry partial-blend ticks retain that
- velocity and suppress the ground-jump request. The head component uses the
- primary-material/level comparison documented in swimming-trigger.md, not
- the steering guard's broader water-or-lava material boolean.
- The jump regression freezes all native float values through entry and exit,
- distinguishes wet and dry suppression, exercises native head sensing, and
- verifies that correction replay reconstructs the same gate.
- MobMovementClimbOutOfLiquid's current callback is **0x09004df0**, identified
- by its current adapter in `09.cpp` line 49309 and the named older body
- **0x06401d40**. Its filter requires LiquidTravelFlagComponent and
- HorizontalCollisionFlagComponent; it applies to water and lava independently
- of swimming/crawling pose.
- `(starting_y - current_y) + 0.600000024 + retained_velocity_y` vertically.
- Matching PE instructions perform each subtraction/addition and face translation
- in float. The raise constant is at VA `0x14ffab698`. It first checks primary
- The executable confirms BlockSource virtual slots `0x38` and `0xa0`.
- The matching vtable at `0x150161770` selects **0x031a7a20**
- (containsAnyLiquid, through its thunk) and **0x031a4560**
- (fetchCollisionShapes, with boolean one). The liquid test floors minimum
- faces, ceils exclusive maximum faces, reads primary getBlock material, and
- does not compare rendered liquid height or inspect secondary storage.
- resolver's actual AABB. Previously the walking probe reconstructed a standing
- box and swimming never received the escape impulse. The dedicated regression
- checks a low pose below an overhead obstruction, both liquids, primary versus
- secondary material, unavailable/conflicting probe rollback, and native float
- impulse. Dry retained swimming uses ordinary acceleration and drag in its low
- box: TravelTypeSensing **0x09fefcb0** selects WaterTravel by WasInWater, not by
- actor flag 57. These synthetic checks do not establish live server acceptance.
- Jump-controller and swimmer-specific paths remain separate. Its one-shot alternate liquid impulse is
- `0.0280000009` at PE VA `0x150376a10`. Those branches are not modeled by the
- ordinary fully submerged regression.
- optional WaterMovement alters unsprinted drag. Those component paths are not
- included in Cinnabar's input contract.
- are implemented from **0x0a5d5c40** and **0x0395d2f0**, with replay-captured
- preceding contact pose and flying policy. The source evidence and exact
- material getter boundaries are recorded in [liquid-currents.md](liquid-currents.md).
- Compile and live acceptance remain pending for this integration. Native
- directional obstruction facts that have not been established remain explicitly
- unavailable; mounted ownership and non-player liquid probes are separate paths.
- swim-entry/exit timing, surface behavior, bubble-column forces and waterlogged
- obstacles still need controlled vanilla and real-server comparisons.

## docs/reference/nametag-rendering.md
- # Native name-tag rendering rules
- The installed PlayCover app and the IPA named `Minecraft-1.26.50-for-iOS-mcpelife.ipa`
- contain the definitions in `data/resource_packs/vanilla/materials/ui3D.material`, lines
- 224–345, and readable Metal shaders in `data/renderer/materials/Nametag.material.bin`
- and `UIText.material.bin`. Their **internal version is 1.26.51.01**, not the filename's
- 1.26.50; they corroborate the material rules but are not a version-matched shader-pack witness.
- SHA-256 values: `ui3D.material`
- `2ca6efaa1e93d650c2476025cb4d6043a70a14516ae542ffcb4218dc8abeeaec`,
- `Nametag.material.bin`
- `cf0f7d60b85c42324fa2955c50a599405bdbaf238c0d800b9eeeefb17ff98d49`,
- `UIText.material.bin`
- `b1f6dc57ea38b7ececf55ec1209b5521e005e192bf05fe0b08ee6ea01e85eb2b`.

## docs/reference/player-preview-rendering.md
- same helper after its independent hand-frame offset. `_rebuildItem` uses mesh offset `(-.5,-.5,-.5)`, centering the cube basis. Native special-shape/custom presentation branches remain distinct.
- the authored default root origin with Y pivot minus the shared ModelPart height.
- pose VM and literal preview share `MODEL_PART_ORIGIN_Y` with the existing GUI ModelPart basis.
- `DataDrivenRenderer::render`
- uses Y rotation `wrap(180-body_yaw)` and then negates the first two matrix columns.
- steps. `animation.player.bob` uses `cos(life_time*103.2)*2.865+2.865` in degrees. The `PaperDollRenderer` constructor sets `variable.is_paperdoll=1`; the install-fetched

## docs/reference/player-skull-lighting.md
- and light-coordinate rules. The native installed client is a near-version
- | Light coordinates | Read `BlockSource::getLightColor` with minimum block light zero. Divide the two retained brightness levels by 16, sample `LightTexture::getColorForUV`, and publish RGB `TILE_LIGHT_COLOR`. |
- | Registration | Register `player_head` and its six companion head types as SkullBlock. |
- | Light sampling | SkullBlock sets light filter zero. Read the requested cell’s retained nibbles directly without choosing neighbouring cells. |
- native shading polynomial represented by `render_api::ACTOR_SHADE_COEFFICIENTS`,
- the correctly lit block-actor geometry. This is a separate native-head mismatch;
- heads. The correction derives these behaviors from the native material path;

## docs/reference/shield-blocking.md
- `query.blocking` reads `ActorDataFlagComponent` (type hash `0xc67426f3`),
- byte 9 bit 0, which is actor flag 72, and returns a boolean script argument.
- It does **not** call `Player::isBlocking`, derive blocking from the processed
- sneak state, or apply a local five-tick timer.
- `ServerPlayer::normalTick`
- sets flag 72. Its eligibility checks include cooldown, sneak/using state,
- Current `SneakTriggerSystem::doActionTick` updates the
- processed sneak/swim/crawl flags but does not set blocking 72.
- The separate damage-blocking predicate `Player::isBlocking` requires flag 72, an active Shield
- than four. `ShieldItem::inventoryTick` maintains that
- timestamp on the server only. `readUserData`/`writeUserData` transmit its trailing signed 64-bit value.
- `ShieldItem::use` is a no-op: starting ordinary ranged-item
- fixture therefore cannot establish a native Shield gameplay or animation
- 72. No client-side fallback bypasses the native metadata query.
- These are functional and rendered-frame checks, not a matched native gallery.

## docs/reference/shield-inventory-icon.md
- | Shield model | Load the authored `shield` ModelPart with default geometry and `ui_shield.skinning` material. |
- | ModelPart orientation | Use `24 - pivot.y` with box-local inverted Y and apply the supplied model unit. |
- ModelPart geometry uses `1/16` units. Matrices post-multiply, so the Y rotation acts first.
- | ModelPart Y orientation | `24` |
- shader behavior, custom rotated/animated/inherited ModelPart trees, exact hardware-MSAA

## docs/reference/swimming-trigger.md
- The target is the canonical reconstructed **1.26.50.26 preview client**, Lens
- artifact 6, executable SHA-256
- `7d6cf9b2e4b01fce5d6283cc3deb65b877995a8fd1e146f967d8ac743369d628`.
- The older named 26.30 reconstruction identifies systems; formulas, current
- dispatch and constants were checked in the current reconstruction and matching
- PE. The code in Cinnabar is independently written.
- ## Current identities and ordering
- `SwimTriggerSystem::doTick` is current **RVA 0x09fd25a0** (named older body
- `0x053bf000`). Current ticking adapter **0x09fe1160** supplies this callback;
- its assertion signature identifies the system and its component arguments.
- Registration **0x072b6020** installs it through category-one registration,
- before horizontal-pose and vanilla-offset updates.
- Current category-one registration proceeds through **0x072ce0d0**, which
- registers `CurrentSwimAmountSystem`, then calls **0x072c5ec0**. That registers
- `InWaterSensingSystem` and `UnderWaterSensingSystem`, then calls **0x072bb110**.
- The latter calls **0x072b6020** to register player input, swimming trigger and
- pose updates before registering jumping and movement. The common registration
- routine is **0x070be000**. The collection appends identities in **0x028e64f0**
- and its ticker traverses forward in **0x028e8f00**.
- ## Native start and continuation
- Entry requires the head-water component, flight disabled, swimming clear,
- sprint intent (`PlayerInputRequest +0x08`) and clear sprint-direction rejection
- (`+0x0a`). The direction rejection comes from current sprint intent
- **0x0c5b6310**. Its ordinary desktop direction checks require movement magnitude
- at least `sqrt(0.5)`, positive forward input and absolute sideways input at most
- `sqrt(0.5)`. A separate native stall check compares retained position/input with
- the current position, using `0.00005`; Cinnabar does not yet retain that complete
- the hunger-stop request (`+0x10`) is clear, the player is unmounted (`+0x0b`),
- the desktop/touch sprint cancellation (`+0x09`) is clear and body water contact
- is present. This continuation does not require the actor's sprint flag or
- positive forward input. The hunger request is produced in **0x0c5b1b60**:
- missing hunger or hunger at most six requests a stop when flight permission is
- absent. The application uses the existing shared hunger threshold.
- the swim. For air at attach seven, native retains the swim when
- surface behavior. The native lookup uses the float sine table for the negative
- When continuation fails, native emits stop-swimming only if the standing fit
- boolean (`+0x0c`) is true. There is no unconditional grounded stop in this
- function. A blocked standing probe therefore retains swimming and its low box,
- including on land; dry travel must still apply ordinary land/air forces.
- Cinnabar formerly relabeled this condition as crawling and also ended a swim
- whenever sprint or forward input cleared.
- Current sprint intent **0x0c5b6310** skips its stop action when the preceding
- swimming flag and current body-water flag are set. This preserves an existing
- sprint through backward/sideways movement, sneak and released sprint input;
- ordinary valid starts remain possible. Because sprint intent precedes swimming
- trigger, even the first stop-swimming tick retains sprint while the old box is
- wet. Cinnabar retains the actual flag with its controller frames, applies the
- same selection during correction replay, and shares that flag across water drag,
- `UnderWaterSensingSystem::doUnderWaterSensing` is current **0x09fd7c00**,
- dispatched by **0x09fd75f0**. It samples attach location seven at interpolation
- zero, requiring the primary water material. Its strict eye comparison uses
- then performs the native float comparison. The application passes its captured
- Bounding-box input update **0x09eeeb70** writes the standing/sneaking/low fit
- flags. Its standing probe uses the existing feet and standing collision height,
- with all faces inset by `0.01`. `sim::pose_fits` now shares this rule; its old
- Pose-size transform **0x02c33550** sets horizontal-pose height to collision
- width. Applying the request in **0x02c33600** preserves the previous AABB minimum
- Y and changes maximum Y to minimum Y plus requested height. It does not shift
- the feet or write StateVector Y. Vanilla offset affects the attach/camera point;
- standing-up itself does not justify changing the network feet anchor.
- ## PE constants and remaining scope
- | PE VA | Float | Consumer |
- | --- | ---: | --- |
- | `0x14feff2ac` | `0.707106769` | Swim and sprint movement threshold |
- | `0x14ffab6c8` | `0.150000006` | Upward swim-entry target limit |
- | `0x14ffd5070` | `57.2957764` | Native radians-to-degrees multiply |
- | `0x15013adc8` | `45` | Surface continuation angle |
- | `0x1503dcba0` | `0.0000499999987` | Sprint direction stall comparison |
- | `0x150064950` | `9` | Head-water liquid-level divisor |
- | `0x150167290` | `-0.111111112` | Head-water level offset |
- | `0x1500c2b70` | `0.00999999978` | Fit probe lower-face inset |
- | `0x1500dfa68` | `-0.00999999978` | Fit probe upper-face inset |
- The focused regressions cover native source/flowing head heights, primary-layer

## docs/reference/third-person-camera.md
- `CameraAPI::tryGetActorInterpolatedPosition` passes its render
- fraction to `VanillaOffsetSystem::getCameraPosition`. The latter
- starts from `Actor::getInterpolatedRidingPosition` and applies the interpolated
- eye/stance offsets. `Actor::getActorToWorldTransform` starts from
- the same interpolated riding position at the same render fraction.
- `LevelRendererPlayer::bobView` independently interpolates walk
- `CameraAttachSystem::_handleLookInput` uses polar/elevation and
- azimuth input, in that order. Its `invert_x_input` flips the polar component;
- pitch when moving to the opposite side of the subject. The look-at update
- (`CameraLookAtSystemUtil::_lookAtSystem`) uses global Y as its

## docs/tracking/vanilla-parity-gaps.md
- `animation.player.first_person.*` (public samples) + `ItemInHandRenderer` transforms.
- | 0 / 92 flags | yes | every `is_*` query, on-fire camera overlay, invisible body (NoDraw; armor and held items stay, as `shouldHideHeldItems` returns false), show/always-show name, sneak tag dimming, sleeping, riding layouts (saddled, baby, tamed, sheared) |
- Missing presentation that the flags drive: the entity flame billboard (`ActorRenderer::renderFlame`)

## plan.md
- Current TopSnow routes through ordinary terrain into AO/flat lighting. The real model-fragment Metal regression reproduced
- ice was CUTOUT despite source alpha 190/255. Native IceBlock selects
- local, uncommitted, not pushed): current LeavesBlock and
- Current TextureAtlas::updateTextureAtUVs and _buildAtlasMips establish
- orientation and sampler contracts. Current BlockType::getShadeBrightness
- AmbientOcclusionCalculator averages outward/side/diagonal shade and raises
- close, not a Rust panic. Named vanilla
- CraftingContainerManagerController::_makeCreateItemScopeCreative
- and the result-action constructor declare the
- vanilla Wolf::getTailAngle behaviour: angry overrides tame,
- not pushed): current SeasonsRenderer palette generation and native
- BlockReplaceableComponent coverage beyond the identified native ground plants
- Native animateTick/LeavesBlock ambient particle sampling, independent fixed-tick
- and broader block animateTick callbacks remain incomplete. Falling-leaf parity
- iOS 1.26.51 item archive; current Item::initClient performs
- component-icon loading. This is labeled a near-version asset witness, not an
- the named Geometry parser and official schema; real pig/cow/sheep model tests
- hotbar and hand, connected cold pig/cow bodies, and snowy grass sides. Current
- GrassBlock::calcVariant selects the alternate side for TopSnow,
- appends the sheared base head's cubes. Native Geometry::_parseBones appends cubes unless reset is authored. The shared inherited-cube
- now retains snow occlusion and the plant's own render layer. Native TopSnow uses terrain tessellation and separate visual bounds.
- Vanilla `LevelBuilder::tryRebuild` uses a radius-16 X/Z availability check:
- unbound `$vars` in `ignored`/`requires` read as null like `UIEval::evalVariable`.
- The pack confirms chat notification 10s and toast notification 3s defaults. Current OptionRegistry defaults remain unconfirmed. FOV, gamma, sensitivities, FPS limits and added boolean defaults therefore
- selected item is that block (`LevelRendererCamera::render`); those layers are not built.
- `getCloudColor` day/weather/sunrise colour with alpha 0.7, drift 0.02
- boat and horse "client predicted" systems (`SetIsClientPredictedBoatSystem`,
- `SetIsClientPredictedHorseSystem`) plus boat paddle/move/friction systems, and a boat's
- `setIconIfLegacy` call in the 26.30 client's `VanillaItems::initClientData`, plus
- potion icons by aux, each row citing its call site. Still open: cooked foods and
- candles, chains) draw `BlockItem::getIconInfo`'s icon: carried texture, else
- slots and use durations. Item glint (provisional): stacks `Item::isGlint`
- marks (an `ench` list, the glint component, always-glinting vanilla items)
- OptionRegistry numeric defaults remain provisional pending current-client confirmation. Cloud and hand preferences do not modify the JSON-UI engine.
- world-item, or entity-glint parity. Current OptionRegistry defaults/ranges remain open.
- Vanilla selects creative destruction from `Actor::isCreative`, not that
- Spawn/motion vectors and interpolation follow the native StateVector velocity
- Current SkullBlockRenderer reads light at the placed skull's integer

## tools/jsonui-editor/src/lib.rs
- // `SceneFactory::_createSafeZoneSizeVar` with a full safe zone: a zero

## tools/registrygen/block_v2193_light.go
- // DynamicLiquidBlock final registration has
- // no version gate: still and flowing types differ.
- Sensor state controls emission independently of opacity; the pinned PMMP property table gives both sensor types opacity 0.19999998807907104, preserving light filter 3 during palette regeneration.

## tools/registrygen/block_v2193_light_test.go
- // 1.26.50.26 TopSnowBlock sets dampening to zero; the
- // inherited getter and per-height component override do not change it.

## crates/render-model/src/entity_shadow.rs
- Volume mesh: `PrefabMeshGenerator::buildShadowVolume` (13 segments, rings 0.25 at y −3 and 0.75 at y 0.01; scaled per instance by radius in `_insertVanillaShadows`).
- Colour: `ShadowColor` uniform built in `LevelRendererPlayer::createViewRenderObject` from `DimensionClientUtils::getInterpolatedSkyColor` and `getSunriseColor` (constants 0.5/0.4 tint, Rec. 709 luminance, 0.7 grey, 0.03 span).
- Blend and overlap: `ShadowVolume` back/front stencil passes then the `ShadowOverlay` pass (`shadow_back`, `shadow_front`, `shadow_overlay` materials).

## crates/client-world/src/actor_store/shadow.rs
- Radius: `Actor::getShadowRadius` (AABBShapeComponent width) and overrides on Ghast, HappyGhast, Creaking, Spider (CaveSpider), EvocationFang, Armadillo, Horse, EnderDragon, Tadpole, IronGolem, Shulker, Turtle, Slime (LavaSlime, SulfurCube), TripodCamera, EnderCrystal, Boat (ChestBoat), Parrot, Player, and the zero-radius classes (ArmorStand, AreaEffectCloud, FishingHook, Minecart family, ExperienceOrb, LeashKnot, EyeOfEnder, LightningBolt, PrimedTnt, FallingBlockActor, FireworksRocketActor, Painting).
- Admission: `createViewRenderObject` caster loop: `isAlive`, radius > 0, `!isOnFire`, `!isUnderLiquid(Any)` at attach location 7, `!isInvisible`, ActorType projectile bit, vehicle `isInvisible`.
- Drop: `RelativeShadowOffsetComponent` (Ghast −0.875, HappyGhast −0.5) × ActorDataBoundingBoxComponent height × scale.
- Projectile identifiers: `VanillaActorRegistryAnon` factory table (types with bit 0x400000).

## crates/sim/src/simulator/water.rs
- `sample_liquid_submersion`: `ActorMobilityUtils::isUnderLiquid` with MaterialType Any.

## app/src/player_skin/catalog/default_capes.rs
- Cape names and public texture identities were discovered through [MCProfiles' official cape catalog](https://mcprofiles.me/capes/official), including its Minecon, promotional and Realms MapMaker entries. These are runtime image inputs for the custom wardrobe, not a claim that an account owns the corresponding vanilla entitlement.
- Original PNGs come directly from Mojang's `https://textures.minecraft.net/texture/{texture_id}` endpoint. Admission pins the actual encoded PNG SHA256 separately from the texture identity, verifies the PNG and checks its dimensions against `protocol::CAPE_DIMENSIONS`; no cape art is embedded in the repository.
- Direct primary-host witnesses include [MINECON 2011](https://textures.minecraft.net/texture/953cac8b779fe41383e675ee2b86071a71658f2180f56fbce8aa315ea70e2ed6) and [15th Anniversary](https://textures.minecraft.net/texture/cd9d82ab17fd92022dbd4a86cde4c382a7540e117fae7b9a2853658505a80625). Modern PNG responses can have a content digest different from their texture URL identity; requesting the content-digest alias returns 404, so the loader must retain both identities.

## crates/client-ui/src/ui_runtime/presentation/player_preview/cape.rs

- Runtime `skin_packs/vanilla/geometry.json` from installed Bedrock 1.26.51.01 supplies `geometry.cape`: texture 64×32, box origin [−5,8,3], size [10,16,1], pivot [0,24,3], Y rotation 180°, parent `body`. This is a near-version geometry witness, distinct from the pinned pack.
- Pinned `bedrock-samples` v1.26.50.4 `animations/player.animation.json`, `animation.player.cape`, supplies the resting X rotation −6° and the movement expression `lerp(0,−126,cape_flap_amount)−6`; `entity/player.entity.json` supplies model scale 0.9375 and the cape geometry alias.
- The shared entity-cube builder supplies the rectangular UV net. Preview meshes retain original cape pixels, attach to the torso transform, and share model depth. Cape gallery cards deliberately use an unobscured back view under the user's custom Dressing Room design scope.
- Current preview input supplies a resting cape, without movement flutter, the armor-neck locator offset, or elytra hiding. Those behaviors and world-actor cape rendering remain separate parity work.

## crates/client-presentation/src/presentation/cape.rs

- Installed Bedrock 1.26.51.01 `skin_packs/vanilla/geometry.json` gives the separate `geometry.cape` shoulder pivot [0,24,3], while `geometry.humanoid.customSlim`'s virtual `cape` marker has [0,24,-3]. Classic `geometry.humanoid.custom` uses [0,24,3]. The marker's animated delta must be retargeted onto the separate cape geometry rather than copied as a completed world translation.
- The pinned pack's `models/mobs.json` supplies the carrier cape hierarchy, cube, pivot and Y-180 bind orientation. `animations/player.animation.json` supplies `animation.player.cape`'s movement X rotation and neck-locator translation. Runtime rig rest transforms are the source attachment frame; current/previous posed transforms supply parent rotation, scale and animation deltas.
## crates/client-ui/src/ui_runtime/presentation/forms/oreui/play/tabs.rs; crates/client-ui/src/ui_runtime/presentation/forms/oreui/paint/typing.rs

- Installed PlayCover `1.26.51.01` hbui is a near-version artwork/layout witness; matching-version acceptance remains open. `data/gui/dist/hbui/index-168bae443ec79c00823c.js` identifies the Play bar through `Y7`, `Q7`, and the `V_` icons; `index-800b52fb984b5ed54515.css` and `menus-theme-b1a483c329eea7188853.css` supply border-image geometry.
- Play tabs use `assets/tabBar_neutral_{default,hovered,pressed,default_focused,pressed_focused}*.png`. Raised faces slice 2/2/4/2 texels into 0.4/0.4/0.8/0.4 rem edges; pressed faces slice 2 texels on every edge into 0.4 rem. Focus adds a one-texel white outset. Raised height is 4.8 rem; selected/pressed height is 4.4 rem with a 0.4 rem top offset. The selected indicator is 4.8 rem wide and one GUI texel tall, just below the face.
- `UI_Menu_WorldsTab`, `UI_Menu_RealmsTab`, and `UI_Menu_ServerTab` are 24×24 images. Small `V_` uses 24×base1Scale and a 0.8 rem spacer before the body label, with the shared icon highlight sheet. Owner-authorized motion shortens the highlight to 200 ms and adds 75 ms typing reveals/caret travel. Caret blink uses the existing JSON-UI interval and restarts on caret revision changes.

## core/catalog/servers.go; crates/client-ui/src/ui_runtime/presentation/forms/oreui/play_servers.rs

- Current 1.26.50.26 `src/__unmapped/0c.cpp`, third-party world-list bindings at `0x0c822600`, expose separate `featuredExperiences` and `creatorExperiences` facets. The repository's discovery-blob path is distinct from the flighted server-tab layout API. Current live discovery returns six ranked experiences and seven creator entries; rank presence identifies featured entries, including rank zero. The older `ThirdPartyUtil::sort3PPServers` witness in `reference/26.30/src/by-owner/_/_1--b3955df4611b.cpp` supplies descending rank comparison. Creator ordering is randomized per catalogue refresh; matching-version ordering acceptance remains open.
- Installed PlayCover 1.26.51.01 hbui is a near-version layout witness. `Iae`/`Oae` and `xae` supply transparent idle list items, 40×base1Scale bordered icons, optional real MOTD captions and separate group headings. `yne` supplies the 4/8 desktop and 3/5 narrow columns. `gne` supplies a 10:3 banner, a 6 rem metadata strip, neutral80 name/Play and description sections. `Dae` supplies 15.2 rem activity images and wrapped title/subtitle/body text; activities precede `Wae` news. `Uae` shows current player count beside the player-online icon; `Vae` supplies ping tier artwork and pending animation. The owner's native screenshot is the visual witness for MegaSMP banner, group membership and detail sections.
- Artwork roles use discovery image tags independently of image type: Icon, Banner and activity ImageTag. Localized metadata falls back to NEUTRAL. Legacy advertised host/port entries retain RakNet ping and direct joins; entries without an endpoint resolve their experience target at join time.
## Experience player counts

- `crates/client-ui/src/ui_runtime/presentation/forms/play_screen.rs`, `crates/launcher/src/menu/view.rs`,
  `app/src/menu/account_control.rs`: installed 1.26.50.04 OreUI bundle
  `data/gui/dist/hbui/index-168bae443ec79c00823c.js`, `Qoe` (offset 1754136) selects
  `vanilla.menus.playerCountsQuery.playerCounts` by experience ID, substitutes zero for missing,
  and mounts the raw numeric count beside the player icon only when positive. `ere` and `tre`
  experience listing cards have no count. `Mn` subscribes at mount and disposes at unmount.
- `core/launcher/service.go`, `app/src/menu/launcher_account/feeds.rs`: current 1.26.50.26
  `src/__unmapped/0c.cpp`, `FUN_14cfec9b0` constructs the query and calls `FUN_14cfed4a0`
  immediately; `FUN_14cfee1e0` refreshes after the service's request age reaches 300 seconds.
  `src/__unmapped/05.cpp`, `FUN_14542d410` caches counts for 300 seconds from request start;
  `FUN_145485de0` leaves the previous cache intact on failure and defaults absent count fields to zero.
  `src/__unmapped/0d.cpp`, `FUN_14d002a30` retains published counts on failure and replaces them on success.

## core/catalog/profile_statistics.go

- Typed gophertunnel userstats batch: current 1.26.50.26 `src/__unmapped/01.cpp:970426` imports `XblUserStatisticsGetMultipleUserStatisticsForMultipleServiceConfigurationsAsync`; `00.cpp:1129438` builds the four-stat request for every configuration, `1192663` selects names, and `1192739` sums doubles across configurations.
- Retail configuration order: `reference/26.30/src/__unmapped/03.cpp:27900-27971` initializes `BEDROCK_XBOXLIVE_ALL_SCIDS` as Kindle, Google, iOS, Xbox, Windows, Switch, Berwick. Installed release 1.26.50.04 binary strings corroborate the seven IDs; its bundled XboxServicesAPI framework identifies `XboxServicesAPI/2025.10.20251000.0`.
- The batch wire schema and headers also match Microsoft's Xbox Live SDK `Source/Services/Stats/user_statistics_service.cpp` and `Source/Services/Common/http_call.cpp`; successful authenticated live requests were not captured.

## crates/protocol/src/ui/commands.rs
- Command-name suggestions use substring matching, as shown by the vanilla command-completion recording attached to issue 220: https://github.com/user-attachments/assets/8fc14920-47a1-4b4e-a57a-99f31e84ef83. Current `CommandRegistry::autoComplete` owns command-name candidate selection.

## crates/client-ui/src/ui_runtime/presentation/gui_scale_settings.rs
- Captured pointer motion follows `SliderComponent::receive` and `_updateSliderFromPosition` beyond the track's hover bounds; only button release ends capture.

## crates/client-ui/src/ui_runtime/event_apply.rs
- Text packet localization uses `Localization::_get` percent-token expansion, followed by parameter formatting; the pinned `texts/en_US.lang` entry `multiplayer.player.joined` is `%s joined the game`.

## crates/client-ui/src/ui_runtime/presentation/primitives.rs
- Translation and command-output rows apply the same `Localization::_get` expansion to marked keys and their arguments before formatting. Parameter formatting still expands percent escapes when the argument list is empty.

## crates/client-ui/src/ui_runtime/raw_text_resolution.rs
- Current 1.26.50.26 game-mode feedback builds `gameMode.changed` with a parameter vector through `TextObjectLocalizedTextWithParams` (artifact 6: `0xcaf2a70`, `0x51cdef0`, `0x5214cb0`, `0x34b0d00`, `0x34b1110`). Translation arguments pass through the I18n parameter formatter; ordinary rawtext text objects remain literal. The named `TextObjectLocalizedTextWithParams::asString` counterpart resolves its child strings before parameter formatting.

## crates/inventory/src/inventory_ledger/crafting.rs
- Creative output preserves every block runtime identifier bit in the declared prototype, including high-bit hashed IDs. The pinned protocol's signed stack field and unsigned craft-result field name the same identity.

## crates/render-model/src/equipment/attachable.rs
- Bound attachable roots use the parent's model-part origin: model-space pivot Y minus 24 pixels before applying hand rotation and scale. The same origin is used by `ActorAnimationController` binding expressions and the pinned `geometry.shield`/`geometry.trident` models.
- Owner-name bindings clear the model-part defaults; unbound roots keep their authored hand-relative origin. Only explicit binding expressions retain the shared humanoid origin.
- GUI held geometry consumes that resolved pose without another origin subtraction; only its original bind pivot is removed when transforming vertices.

## crates/client-ui/src/ui_runtime/presentation/player_preview/equipment.rs
## crates/render/src/ui.wgsl
- Retail 1.26.50.4 `definitions/attachables/leather_helmet.player.json` selects `armor_leather`; `materials/entity.material` inherits `entity_alphatest_change_color` with `USE_COLOR_MASK`. `shaders/glsl/entity.fragment` discards only alpha zero and uses texture alpha as the original-versus-dyed RGB mask.

## crates/pack-compiler/src/entity/item.rs
## crates/pack-compiler/src/icon.rs
- Pinned `textures/item_texture.json` supplies extensionless leather icon sources. Helmet, leggings, boots and horse armor are `.tga`; chestplate is `.png`. Retail color-mask material rules preserve the low-alpha original-color trim and make all surviving texels opaque.

## crates/meshing/src/liquid.rs
- Current `BlockTessellator::tessellateLiquidInWorld` (`0x06a1b960`, artifact 6, 1.26.50.26) reads extra-layer air for classic side/bottom admission and primary-layer air for reverse winding. `BlockTessellatorCache::getExtraBlock` and `getBlock` in the 26.30 reference corroborate the two distinct virtual slots.

## crates/assets/src/banner.rs
## crates/assets/src/block_entity_geometry.rs
## crates/assets/src/gui_item.rs
## crates/pack-compiler/src/icon/block_entity.rs
## crates/pack-compiler/src/icon/bake.rs
- Current 1.26.50.26 `BannerModel` constructor `0x1e6da00` authors a 20×40×1 cloth at `(-10,0,-2)` with model-part Y pivot −32, a 2×42×2 pole at `(-1,-30,-1)`, and a 20×2×2 crossbar at `(-10,-32,-1)`; UV origins are `(0,0)`, `(44,0)` and `(0,42)` on the 64×64 banner base texture.
- Current banner GUI renderer `0x6c581a0` uses `T(8.5,11,-10) * S(5.5) * Rx(20°) * Ry(-30°)`, model units 1/16 and static cloth tilt zero. The frame is uncolored; only the cloth uses the base dye.
- Current banner constant setup `0x6c57b00` reads `ItemColor` RGB entries from the table at `0x10128cd4`, black aux 0 through white aux 15 (`#f0f0f0`). `BannerItem::buildDescriptionId` `0x29b4620` corroborates identity aux-to-color mapping for values 0 through 15. The old sign atlas route does not represent the GUI banner model.

## tools/registrygen/education_v2193.go
## crates/pack-compiler/src/compiler/visuals/literal.rs
- Pinned 1.26.50 palette `block_states.nbt` contains allow and deny with empty state, plus 162 border states (four wall connections in none/short/tall and byte wall_post_bit). Vanilla packs route them through `build_allow`, `build_deny`, and `border_block`.
- `BorderBlock` inherits `WallBlock`; `BlockGraphics::initBlocks` registers it as shape 32 and `BlockTessellator` dispatches that shape to `tessellateWallInWorld`. Reference 26.30 `by-owner/b/BorderBlock.cpp`, `BlockGraphics.cpp`, `BlockTessellator.cpp`; current 1.26.50.26 border description/shape methods are in `__unmapped/03.cpp` near the `tile.border_block.name` owner.

## crates/assets/data/default-sprite-bindings-1.26.50.json
## crates/pack-compiler/src/entity/item_bindings.rs
- Retail 1.26.50.4 `vanilla/__brarchive/items.brarchive` declares each food's `components.minecraft:icon` key; baked potato uses `potato_baked`, meat uses raw/cooked atlas aliases, golden carrot uses `carrot_golden`, and poisonous potato uses `potato_poisonous`. The installed archive and all four seed definitions match the existing witness hashes.

## crates/assets/src/block_entity_geometry.rs
## crates/pack-compiler/src/icon/block_entity.rs
## crates/pack-compiler/src/icon/blocks.rs
## crates/render/src/block_entity/book.rs
## crates/render/src/block_entity/pot.rs
- Pinned retail 1.26.50.4 `blocks.json` supplies separate inventory front/side/top terrain tiles for chest, trapped chest, ender chest, and each copper chest age/wax variant; placed entity-only visibility does not control their GUI cube route. Bell uses `carried_textures: bell_carried`, resolved through `textures/terrain_texture.json` to `textures/items/villagebell`.
- Current 1.26.50.26 GUI item dispatch (`0x05e57730`, special model selector `0x05e5d740`) sends shulker shape `0x59` to `0x07be6330`, conduit shape `0x65` to `0x06c7f100`, and ordinary block geometry to `0x05e5b2a0`. Shulker GUI uses the ordinary cube matrix with global light `0.73`; `models/entity/shulker.geo.json` supplies the closed base/lid boxes and their 64px texture unwrap.
- Conduit GUI (`0x06c7f100`) uses translation `(8,8.5,-10)`, scale `18.5`, pitch `30°`, yaw `-50°`; the model constructor in the block-entity model dispatcher (`0x06c5e9c0`) supplies the 6px shell and logical 24×12 texture.
- Decorated-pot GUI (`0x06c81c90`) uses translation `(8,9,-10)`, scale `10`, pitch `-150°`, yaw `-45°`. The base constructor (`0x01e87a10`) supplies the 8×3×8 neck with inflation `-0.1`, 6×1×6 lip with inflation `0.2`, and two 14px body planes at heights 16 and 0; the side constructor (`0x01e88410`) supplies 14×16 side panels. The shared centered authoring bounds retain neck heights 14–17 and lip heights 16–17 before inflation.
- Lectern shape `0x73` is registered by current `BlockGraphics::initBlocks` (`0x069f19a0`); inventory tessellation (`0x06aab610`, case `0x73`) uses the default north-facing state and invokes the same lectern tessellator (`0x06a72cc0`) as the placed model. Its native dimensions, four-direction board transform table (`0x150285160`), and texture rotations/crops (`0x150286280`–`0x1502862b0`) define the shared 18 faces: 16×2×16 base, 8×12×8 post, and 15.8×4×13 sloped board. Board pitch is `-22.5°` for north, centered pixel pivot `(0,7,-1)` and offset `(0,1.05,1)`.
- Lectern post front/back uses an 8×13 crop rotated by a quarter turn; the board top uses rows 1–14. Face orientation was checked against the current generic up/north/south tessellators (`0x06a0f830`, `0x06a11a00`, `0x06a13c30`), pivot rotation helper (`0x062fa300`), and transformed vertex emission (`0x062f7760`). The pinned `textures/blocks/lectern_{base,front,sides,top}` files supply those pixels.

## Camera packet family and aim-assist runtime


Root reference corpus: ~/coding/go/lunar/refs/mcsrc-1.26.50/reference/26.30/src
Current corpus: ~/coding/go/lunar/refs/mcsrc-1.26.50/current/1.26.50.26/src

- camera/server_view/runtime.rs defaults/offsets/facing: reference by-owner/c/CameraInstructionSystemUtil.cpp _tick 3940..4360; current __unmapped/07.cpp 229990..230255 independently corroborates same field presence, entity-X negation and default/remove reset rules. Facing thresholds current/source constants: reference 0x10d972260 =0.0001 squared distance, 0x10d972268 =0.01 horizontal distance.
- Preset values and attachment pivots: reference by-owner/c/CameraPreset.cpp and CameraPresets.cpp; CameraAttachSystemUtil::_setPivotPoint applies yaw-relative entity offset; CameraOffsetComponent active/default fields +0..+0x10 and +0x38..+0x48.
- Packet ordering: by-owner/c/CameraPacketUtils.cpp handleInstructionPacket 30..2400: emitted event categories set line537, target829, remove-target1015, clear1172, fade1357, FOV1565, spline1876, attach2142, detach2314.
- FOV: CameraInstructionSystemUtil.cpp 5630..5670; data reads0x10d9b2f9c=30deg,0x10dcc6bb0=110deg,0x10e47ad84=110deg radians.
- Legacy camera: by-owner/c/ClientNetworkHandler.cpp CameraPacket handler14616? actual header14816 then body14817..15035; education guard, uniqueIDs,-1 sentinel, camera item onUse, delayed50ms tripod path. It is photography not view switching.
- camera/easing.rs: Lens function reads easeSpring0x10d822480, mce::Math::easeInExpo0x10d822970, mce::Math::easeInOutElastic0x10d823120; selectors32; spring native sin LUT and phase ((2.5*t^3+0.2)*PI*t), damping(1-t)^2.2, envelope1+1.2*(1-t); elastic period0.3 even inout; expo no endpoint specialcase. sim/math minecraft_sin/cos is existing 65536 lookup implementation now exported for camera reuse.
- camera/server_view/fade.rs: reference by-owner/c/CameraFadeAnimation.cpp addFade/addKeyframe/evaluate; DEFAULT_FADE_VALUES from0x... data lookup done in earlier session: [1,.5,1]. Current __recovered/CameraFadeAnimation.cpp FUN_1471f9970 actually addKeyframe (recovered name evaluate is misleading), corroborates overlap envelope and1e-4 keyframe tolerance.
- camera/server_view/spline.rs: reference CameraSplineUtils.cpp catmullrom0x10c313c50 pointcount-1<3 rejects; linear builder pointcount-1<2 rejects; CatmullRom endpoint half-tangents and normalized edge-distance knots. CameraInstructionSystemUtil::_applySplineFromApi0x10c3409a0, _applySplineFromJson0x10c341e40, curves typebyte0/1; names case insensitive catmullrom/linear. CameraSplineSystem::calculateCameraAnimation0x10c353f80 increases elapsed before strict-duration check, source-keyframe ease, independent tracks; applyCameraAnimation0x10c354890 rawEuler quaternion initial interpretation superseded by current direct YXZ formula (all positive degree-to-radian conversion in _applySplineFromApi). Contrasted by-owner/s/StationaryCameraSystem.cpp _tickSpecificPreset0x10c35c210 lines65..68 explicitly negate pitch and180−yaw forordinaryset; setupCameraAnimation0x10c354d10 initializes tracks. Current __unmapped/07.cpp: inline apply FUN_147163c30, named apply FUN_147164750, Catmull builder FUN_1471fd040, linear builder FUN_1471fd660, calculation FUN_1471756a0, apply FUN_1471760e0 and setup FUN_1471767f0 independently mapped. Apply explicit quaternion factors are Y*X*Z. Current stationary functions FUN_147179450/FUN_147179c90 write default/instruction pose every frame, then spline overwrites while alive; duration crossing removes animation data. Instruction and spline views lack ActiveCamera filters, so inactive stationary cameras keep advancing.
- camera/shake.rs: reference by-owner/c/CameraShakeSystem.cpp _tickComponent0x1058a38d0 lines1..253 queues,sumcap4,decay then maxfloor,expire aftereval,componentremove when bothqueuesempty. Constructor entt::basic_storage<CameraShakeComponent>::emplace_element<>0x1035b1850 initializes +0x24 decay float1.0. Current __recovered/CameraShakeComponent.cpp queueShakeEvent0x142718f00 verifies strictly-positive add semantics; function recovered as getShakeIntensity0x142718ca0 is actual vector sampler(time*param4,param5)*param6*param4*intensity. Current caller FUN_147175190 passes support fields8,0,4 respectively; support metadata FUN_14ea58f40/FUN_14ea59050/FUN_14ea59120/FUN_14ea591f0 registers frequency/amplitude/noise_multiplier; amplitude setter FUN_14ea6d650 converts degrees→radians and getter FUN_14ea6d840 uses57.295776 confirmed data14ffd5070. Thus sample(elapsed*4,10)*radians(5)*4*intensity. Current apply subtracts sampleX/Y from YXZ Euler radians and resets roll.
- camera/shake/noise.rs: current __unmapped/0a.cpp FUN_14a772a80 at1312912 (2D SimplexNoise::_getValue), standard12 projected gradients, skew/unskew, fastfloor<=0 decrement, fourth attenuation,70scale; reference by-owner/s/SimplexNoise.cpp; CameraShakeComponent::initialize creates3 independently shuffled permutations using sharedRandom; local implementation permits independent nondeterministic seeds with equivalent shuffle distribution.
- Native camera definitions: /Users/hashim/Coding/Go/Lunar/minecraft-apk/split_install_pack.apk entries assets/assets/resource_packs/vanilla/cameras/{first_person,third_person,third_person_front,free,follow_orbit,fixed_boom}.json; copied OUTSIDE git /tmp/camera-native-defs.txt. All shake freq10/amp5/noise4. Onlyfree stationary. Allothers camera_offset. Follow/fixed radius10,starting_rot45/45. Third radius4. Explicit starting rotation, selected-only inheritance opt-in and suppressed starting state are covered by the camera regressions. Pack manifest is adjacent1.26.31.1, not pinned1.26.50. Current argument/formula references remain independently pinned.

- Target center-offset yaw rotation: reference CameraInstructionSystemUtil::_applyTarget lines316..324 copies offset unchanged; CameraTargetSystem::doDefaultTarget0x10c357200 lines160..210 rotates XZ using native sine lookup x*cos−z*sin,z*cos+x*sin, then adds actorposition. Native actor-location default/attachment interpolation remains a separate rig fidelity limitation.

- Clear resets FOV: current __unmapped/07.cpp231504..231622 constructs a clear FOV instruction then FUN_147163930 for all cameras before releasing active state; old CameraInstructionSystemUtil::_tick5964..6080 removes customFOV.
- Target defaults current FUN_147da1f40 (__unmapped/07.cpp2105564): zero24-byte settings then50.0f@8. Thus snapfalse,continuefalse,speed0,maxdistance50; free pack supplies0..360/0..180limits. Current registry FUN_140a93e80 directly copies options. Current initial target FUN_14716f830 corroborates yawcenter initial−.5*(hmax−hmin), verticalends−90. Old targetapply0x10c340140 creates positive-speed component and halfwidth. CameraTargetSystem::doDefaultTarget0x10c357200 applies1024hardrelease,distance50/outofangle range,actor removal; CameraTargetRotationSystem::targetWithRotationSpeed0x10c355120 and resetTargetCameraWithNoRotationSpeed0x10c3553e0 define snap/continue/rotateTowards.
- Follow/fixed input/pivot: CameraAttachSystem::_handleLookInput0x10c316a00 yawwrapandclamps; fixed defaultpivot0x10c3162a0 adds90tofixedyaw. Current registry yaw limit validation FUN_140a93e80 constants DAT14ffab65c=-PI/DAT14feff2c0=PI, degrees DAT14ffab66c. Native namedpreset rejects radius,yaw,starting changes.
- Starting flag current registry __unmapped/00.cpp2004374..2004789 copies selected apply_inherited flag@0x120 and optionalstarting@0x124/12c without merging into ancestor loop, then adds IgnoreStarting through FUN_140ac1740 if both absent/false. Current orbit constructor FUN_14097aeb0 zeros yaw/polar34/38, definition copiesonly<=21thenradius24/30; remainingactivation initialstate unresolved.
- Collision CameraAvoidanceSystem::_tick0x10c31cea0 (oldreference)8raycorners; constants10d9fdc10/10d9e1f90=.1 confirmed. Nearclip CameraDefinition currentctorFUN_147d820a0=[80f,.025f,2500f], currentdefinitionadapterFUN_1409762c0 writesnear54, old0x009e2db0 writesnear4c usedbyoldcollision. Adjacentpack distance_constraint_min=.25. Currentcollisionbody corroboration pending.
- Player effects getter current FUN_140a93e80 optionaltruecallsFUN_14098a860 whichget_or_emplace<PlayerStateAffectsRenderingComponent>; false cannotremoveintrinsicbasecomponent. Inheritance fieldsmergednearestfirst beforeapply, so explicitfalsecanblockancestortrue onfreebase. Listener optional1addsPlayerAudioListenerComponent, defaultcamera.
- Current collision corroborated: source_search CameraAvoidanceSystem→adapter FUN_147275a30 callback FUN_1472061e0 (__unmapped/07.cpp332420). Same8corner loop, nearplane param2+54, minavoidanceparam3+34, desired-radius comparison. Current cornerconstant DAT14ffab644=.1f. Native extra azimuth probe array empty in available pack witness; no smoothing spring configured.

## Starting rotation activation and suppression
- Current `FUN_1471b22d0` (`__unmapped/07.cpp`, around278404) gets player view vector via `FUN_14022d030`, calls `FUN_1471b1a60`, and copies the resulting orbit angles. This corroborates reference `UpdatePlayerFromCameraSystem::handleCameraActivation` (`0x007feb40`) and `_updateFromPlayer` (`0x007fe590`): follow-orbit activation starts from player look.
- Current registry constructor (`__unmapped/00.cpp`, around2004374–2004789) saves selected preset apply-inherited flag+0x120 and optional starting rotation+0x124/presence+0x12c before the inheritance walk; neither field participates in inheritance. Custom presets with neither flag nor explicit rotation call `FUN_140ac1740` to add IgnoreStartingValues.
- Current fixed-boom orientation `FUN_14726c5b0` (`__unmapped/07.cpp`, around412056) get-or-emplace initializes its two angle floats to zero. Native first-use starting values remain45/45 from the adjacent pack witness; pinned pack confirmation remains open.

## Inactive targets, range and player-state consumers
- Current target instruction dispatch `FUN_147163150` is called for every view entry with StationaryCamera, target settings and instruction components (`__unmapped/07.cpp`, around231172/231240/231299). Target tick adapter around269289 has `Filter<StationaryCameraComponent>` without ActiveCamera. Remove-target uses the separate ActiveCamera+CameraTarget+CameraInstruction view. Runtime now retains and advances per-preset focus state; explicit removal addresses the active state only.
- Current registry `FUN_140a93e80` (`__unmapped/00.cpp`, around2001470) copies optional preset+0x90/presence+0x94 into CameraTargetSettings+8. It follows continue-targeting+0x8c/+0x8d and precedes view offset+0x98/+0x9c/+0xa0, matching packet field order. The vendor calls this `block_listening_radius`; its consumer is the target range (default50), not the audio listener.
- Current `FUN_140a9c980` is the PlayerStateAffectsRendering capability query. Current fog setup (`__unmapped/04.cpp`, around2430336) gates blindness/darkness on it; current base lightmap builder `FUN_14707f...` (`__unmapped/07.cpp`, around83067) gates night vision/conduit inputs. Compare reference `CameraTraits::usesPlayerState`0x02058980, `LevelRendererPlayer::setupFog`, and `BaseLightTextureImageBuilder`. Nausea/portal projection is not in these capability consumers.
- Current lightmap/fog rendering now consumes a filtered copy of VisionEffects so suppressed player effects resume their current underlying strength immediately when selecting a camera that supports them. Lava fire-resistance fog remains an atmosphere-owner limitation.
- Experimental resource preset witness `behavior_packs/experimental_creator_cameras/cameras/presets/control_scheme_camera.json` in local pocketmine bds-data declares `inherit_from=minecraft:follow_orbit` and `control_scheme=camera_relative`; this is a data preset, not an additional native rig. Version of this pack witness has not been independently pinned.

## Native restrictions and portal capability
- Current `FUN_140a93e80` (`__unmapped/00.cpp`, around2001574/2001581/2001611) rejects starting rotation, yaw limits and radius when the selected name matches the native list. Runtime suppresses those values based on selected name; custom inherited camera names retain them.
- Adjacent shipped camera definitions put `minecraft:camera_portal_distortion` on first_person, third_person, third_person_front, fixed_boom and follow_orbit, but not free. The runtime now gates projection distortion independently of PlayerStateAffectsRendering; an integration regression proves free+player_effects=true stays undistorted and clear restores distortion. Exact-version pack component witness remains open.
- Current boom-definition factory `FUN_147da52f0` (`__unmapped/07.cpp`, around2108055) zero-initializes all16 bytes. Thus its10/45/45 native values are pack values, not constructor constants; constructor evidence does not close the pinned-pack gap.
- Pocketmine bds-data directory has `bedrock_server-1.26.32.2` and no resource camera entity definitions. Its experimental control-scheme preset is an adjacent witness, not pinned1.26.50 evidence.
- Cohesive source owners after root extraction: `camera/settings.rs` retains settings authority; `camera/controls.rs` retains device/freelook/automation control; `camera/rig.rs` owns preset placement and eight-corner collision. Public camera exports unchanged.

## Gameplay FOV capability
- Current capability query `FUN_140a9b4f0` tests GameplayAffectsFovComponent; current CameraAPI call around `__unmapped/07.cpp:274969` switches between the no-gameplay FOV and gameplay FOV getter. Reference `CameraAPI::tryGetFOV`0x007f8220 and `LevelRendererPlayer::getFovWithoutGameplay`0x043a8bb0 identify the latter path as the user's FOV option, not the camera definition's constructor value.
- Free camera definition omits GameplayAffectsFovComponent, independent of player-effects setting. Runtime now masks the gameplay multiplier while preserving its state, with actual projection regression. General gameplay FOV magnitude approximations predate this task and remain explicitly provisional in camera/fov.rs.

- Current clear branch (`__unmapped/07.cpp`, around231599) calls `FUN_14716acb0(param6)` before `FUN_14716a6e0(param5)`, identical to explicit remove-target and detach branches around231100. The target helper iterates the ActiveCamera+Target+Instruction view and calls `FUN_147163070`, preserving the last target rotation in the instruction. Therefore clear removes active focus, while inactive focuses persist. Regression covers retargeting after clear not changing the released preset.

Local reference root: /Users/hashim/Coding/Go/Lunar/refs/mcsrc-1.26.50/reference/26.30/src
Current source root: /Users/hashim/Coding/Go/Lunar/refs/mcsrc-1.26.50/current/1.26.50.26/src
Only add provenance to docs/agents/vanilla-refs-map.md.

- Registry and direct command handler: by-owner/c/ClientNetworkHandler.cpp CameraAimAssistPacket handler around14503; CameraAimAssistPresetsPacket around15191; CameraAimAssistActorPriorityPacket around15270.
- Client actor triple map: by-owner/c/CameraAimAssistActorPriorityClientComponent.cpp RVA0x04ecbd20, upsert semantics and no clearing unrelated records.
- Category: by-owner/c/CameraAimAssistUpdateCategorySystem.cpp RVA0x0230b790, odd tick, empty hand vs item/default category, liquid item list, GameMode::getPickRange(InputMode) virtual+0x58 on player+0x9e8, including touch input2 to controller3 remap when touch option is enabled; return cachedData[0] is squared reach check in result creation.
- Actor metadata triple usage/exclusion and eligibility: by-owner/c/CameraAimAssistFetchValidEntityTargetSystem.cpp RVA0x02305b30. Native requires mob/boat/minecart component, uses HitboxComponent/SubBBsComponent as well as AABB. Native class map and advertised custom constructor classes implemented; additional hitbox geometry still absent.
- Frustum: by-owner/c/CameraAimAssistSystemUtil.cpp createAimAssistFrustum RVA0x0ae963e0; tangent from native SIN lookup, nearest box point and strict distance check used in fetch.
- Occlusion/weight: by-owner/c/CameraAimAssistFilterObstructedEntitiesSystem.cpp RVA0x02306580. Distance mode internal0 includes actor-on-actor box occlusion; angle internal1 does not. Weight clamp constants read_data reference0x10d9f4ff4=-.01,0x10daa8ce8=1.1,0x10d9e1f90=.1.
- Current block-side score independent corroboration by parent: current __unmapped/06.cpp FUN_1467b45c0 RVA0x67b45c0 adjacent RTTI FUN_1467b46f0. read_data artifact6 at DAT15027ee94=-.8726646304130554,DAT1500f2ce8=-.6981316804885864,DAT14feff2b4=1.5707963705062866. Angle cubic and squared weighted distance; strict< ties.
- Reference block-side scorer: support/std/__func--bc8d9908b204/c.cpp RVA0x02357350. Entity scorer support/std/__dispatcher--3e41396f78de/{1,6}.cpp RVA0x02357df0/0x02356fc0 decomp drops FP return; entity score parity needs additional corroboration.
- Block sampling: by-owner/c/CameraAimAssistCaptureBlockTargetPositionSystem.cpp RVA0x022ffa50. Odd clears cache, upper half; even subtracts ceil(halfheight) row offset and adds lower half. Columns2ceil(halfwidth),rowsceil(halfheight), union unique results.
- Position/look cache: by-owner/c/CameraAimAssistCachePositionDataSystemImpl.cpp RVA0x022fe210 odd ticks use local actor attach eye and actor rotation; no rendered camera transform.
- Result schedule/faces: by-owner/c/CameraAimAssistCreateResultSystem.cpp RVA0x023015a0 odd returns prior result except removed actor, even constructs/scans face scores and selects best. Face visibility offset .01.
- Interaction direction: by-owner/s/StrictTickingSystemFunctionAdapter/e.cpp CameraAimAssistUpdateInteractDirectionSystem singleTick RVA0x02395e10 computes targetpoint-cached-eye yaw/pitch; otherwise raw actor rotation.
- Action rotation: by-owner/c/ClientGameModeMessenger--a824a01485dc.cpp tryRotateTowardsAimAssist RVA0x04824400 writes CameraAimAssistRotationOverrideComponent and actor rotation. CameraAimAssistSystemUtil::shouldRotatePlayerOnProjectile ref RVA0x0ae99340 current __unmapped/02.cpp FUN_142d9c860 RVA0x02d9c860 corroborates matrix (0locked-relative,1camera-relative,2camera-relative-strafe,3player-relative,4player-relative-strafe; free/fixed alltrue; follow0false1true3true; unmapped false).
- Action caller confirmation: reference client messenger vtable0x110cb3b50 slot0x60 points0x104824400; read_data confirmed. by-owner/g/GameMode.cpp _attack RVA0x0a0bba20 line638 and releaseUsingItem RVA0x0a0c14a0 line4371 both call messenger slot0x60.
- Activation: by-owner/c/CameraPresetAimAssistActivationSystem.cpp handleCameraActivation RVA0x022fddd0: camera perspective0 unsupported, absent aim option clears, supported+aim callsset(true). CameraAimAssistSystemUtil::isAimAssistSupportedCameraType RVA0x0ae99280;setAimAssistFromClient RVA0x0ae98070 verifies aim registry ID (default minecraft:aim_assist_default), sends selected CAMERA preset name;clearAimAssistFromClient RVA0x0ae991b0 sends empty name,cleartrue,supportbool. ClientCameraAimAssistPacketPayload ctor RVA0x05dba840 clips raw camera name64bytes, storesclearfalse/supportbool.
- Clear packet defaults only: read_data ref0x10dec4ea0=>[30,45], clearAimAssistForServerPlayer RVA0x0ae98df0? line2666 distance5.7. Not used as invented set defaults.


- Block priorities: by-owner/p/PriorityCategory--30ce72cbb0e2.cpp getBlockPriority RVA0xae95760 maximum explicit/tag, absent baseline -1; PriorityPresetExclusionData--221daf0e3956.cpp RVA0xae94ef0 exclusions any exact/tag. BlockType tags +0x170/+0x178; addTag RVA0xac91480. Native constructors TrapDoorBlock/DoorBlock/FenceGateBlock/CropBlock/SweetBerryBushBlock/PitcherCropBlock, custom BlockDefinitionGroup.cpp6682.
- Frustum input axis order confirmed sourcecreateAimAssistFrustum: param3.y=>halfwidth(+0x3c),param3.x=>halfheight(+0x40). right basisY cross forward,up forward cross right.

- Liquid outline: LiquidBlock ctor0xab16690 vtable0x110fcd7b0; read_data +0x40=>BlockType::getOutline0x10ac8d390, +0x50=>getVisualShape0x10ac8d530 returningthis+0x188. Baseconstructor initializesBLOCK_SHAPE; initializer__GLOBAL__sub_I_unity_20_cxx.cxx0x10cea7310 setsmin0max1; read_data0x10d91db40=[1,1],lastmax0x3f800000. LiquidBlockBase::getCollisionShape0xab1b530 iszero-volume and is not outline.
- Aim ray traversal: CameraAimAssistSystemUtil::blockHitDetect0xae95a00 initialprimary-outlineoutside-only, subsequentstepscube whenparam5false,outlinewhenparam5true,liquidextra-first,depthnonzerountargetableonlyprimaryliquid,strictaxisorderingZ/Y/X,integerstepbudget. Acceptance callbacks support/std/__func--bc8d9908b204/8.cpp0xaee4740 rejectair/liquids vs /d.cpp0xaee45e0 rejectaironly.
- Liquid actor visibility end offset: FilterObstructedEntitiesSystem0x02306580 +0.2Y whenliquids; read_data0x10db66110=[0,.2],0x10d91daf0=0.

- Default control schemes: ControlSchemeUtils::getDefaultControlScheme0xafb0ce0 firstallowedunlessinheritedvalidexplicit. Globalinitializer0xafef? Lensentryqueried0x10afefd13 builds free=[0,4,3,1,2],follow_orbit=[0,3,1],fixed_boom=[0,1,2,4,3],first/third/front=[0].
- Native actor eligibility identifier map: ENTITY_TYPE_MAP0x1113c5128 initializedby0x109e037f0 (__cxx_global_var_init.205), paired MOV string bytes/ActorTypenumbers obtainedfunction_inspect. Stringpairctor0x109c8acb0; allnativeclassnames decoded exactly fromimmediatesand20literalvectors. Mobmask0x100,minecartmask0x80000,BoatRideable0x5a,ChestBoatRideable0xda. Constructorclasscorroboration by-owner/v/VanillaActorRegistryAnon--6a61ceb1fa90.cpp0xa23c6a0. Unknowncustom identifiers excluded/counted pendingruntimeclassdata.

- Custom constructors: ActorInfo::load0x9c52940 readsid/bid (ridnotclass). ClientNetworkHandlerAvailableActorIdentifiers0x3551620 feeds ActorFactory::digestIdentifierListFromServer0x9a05b30, insertabsentonly. ActorFactory::fillFactoryData0x9a01750 initializescreateActorFromClass<Mob>,type0x100; knownnativebid overridesconstructor. ActorDefinitionIdentifier::_extractIdentifier0x99fe320 defaultnamespaceminecraft and strips<spawnevent> (lines755–855). Protocolregistry normalizes idlist/id/bid; clientworld retainscustomclasssessionmap throughdimensionchanges. JolyneGameData queuesoptionalentitydefinitions forplay sojoinregistry usesnormalhandler.

- Highlight current corroboration: current1.26.50.26 __unmapped/04.cpp FUN_144edad40 lines2492215–2492535 (result+0x11d nonzero renders; debugcVar19 only constructs/disposesstrings). Description ctor FUN_146878a00; framebuilderinsert FUN_144e3eeb0. RecoveredCameraAimAssistRenderer0x147bf7b90 texturekeys match pinnedinstalledpack. Currentgeometry __unmapped/00.cpp FUN_140a9df40(block),FUN_140a9e210(entity).
- Highlight geometry reference CameraAimAssistGraphics0x20598a0 unitquad UV0,0 at+.5,+.5; blockmatrix0x2059bd0 rotations data0x10db283f8 6x16 floats; cardinalstrictmaxZ,-Z,X,-X fortopbottom; entity0x2059e00 quatLookAtRH(-camera_direction,camera_up). Description tintdata0x10d907ff0=[1,1,1,1]; BgfxFrameExtractor highlight0xbdc1d60 uniformsTextureOpacitydata0x10daa9090=[.5,.5,.5,.5]. The pipeline and two-pass follow-up below supersede this initial single-pass observation.
- Current block matrices independently confirmed read_data artifact6 address0x150079fe0,96f32 identical reference6xmat4. FUN_140a9df40 preserves strictdominantaxis tie order. Current release callbackFUN_144edad40 debugbranch only format/dispose; no drawsubmission forlabels.

- Spectator eligibility: reference CameraAimAssistFetchValidEntityTargetSystem0x2305b30 player path actor+0x251(dead) or vtable+0x128 canInteractWithOtherEntitiesInGame; Player vtable0x110f621c0+0x128=0x10a1e56c0 confirmed read_data. Player::canInteractWithOtherEntitiesInGame0xa1e56c0 returns!Actor::isSpectator; Actor::isSpectator0x9946040 compares explicit6 or default5+world6. Legacy3/4 remain distinct for admission; existing HUD capability mapping preserved. Player branch bypasses category-zero -2 non-player sentinel.
- Tick eye integration: CameraAimAssistCachePositionDataSystemImpl0x22fe210 uses GetAttachPositionUtility location3 and ActorRotationComponent. GetAttachPositionUtility0x6485740, _getBaseAttachPoint0x6485400 reads local offsets. Retained simulation sample/control history supplies tick positions, eye heights, raw player look without render interpolation. Missing historical remote geometry remains an owner limit.
- Cached interaction/action direction: StrictTickingSystemFunctionAdapter/e.cpp0x2395e10 calculates resultpoint(+0x110)-cachedposition on every tick and writes pitch/yaw result+0x120; ClientGameModeMessenger0x4824400 copies result+0x120 to rotation override and ActorRotationComponent. Thus odd ticks retain target identity but refresh direction from odd cached eye; even ticks reuse it. No rendered-eye recomputation.
- Highlight pipeline current table corroboration from root: FUN14f8eecc0 DAT1504abe88 entry2=0x06565000 sourcealpha/inversesourcealpha; FUN14cc1e1b0 DAT150402ac0 entry5=0x50 BGFX strictGreater. Reference description PassState RGBwrites7,depthwrite0,blend2; exact texture sampler default remains unproven and must stay an incomplete parity item.

- Runtime update cadence: ClientNetworkHandler priority0x35497a0 onlysetEntityPriorities; presets0x3549590? sourcearound15191 invokes registryload/update. CameraAimAssistRegistryComponent load0x4ed7ac0 destroys onlypreset/categorymaps thenupdate0x4ed0650. Noentitytick/cachereset. FilterObstructedEntities0x2306580 checks even parity thenreads currentlocalmetadata136137 andtarget138/tabletoassignweight.

- Even final owner validation: CameraAimAssistCreateResultSystem0x23015a0 sourcearound1170 callsViewT<ActorOwnerComponent>::tryGet beforeconstructingactorresult; missingwinningactor clearsresult ratherthanselectingrunnerup. ExistingHashMapactoriterationdoesnotreproducenativeECSorder, soequal-scoreactororderingremainsunproven.

- Highlight two-pass correction: reference support/std/__dispatcher--3e41396f78de/c.cpp BgfxFrameExtractor highlight0xbdc1d60 firstpass18482–18703 setsdepthcomparison5 Greater,TextureOpacityDAT10daa9090=[.5;4]; secondpass18790–190xx depthcomparison2 Less,TextureOpacityDAT10d907ff0=[1;4]. Bothblend2,RGB7,noDepthWrite,bias(description8c-32)=-33,slope0,clamp0. Firstscissortrue,secondfalse. Existing conventionalnativeZ→BevyreverseZ mapping swapscomparisons andbias sign.
- Bias chain proof: refMeshRendererSystem::RenderItem0xc0699c0 lines76–77 GPUState4c/50→EncoderImpla8/ac; EncoderImpl::submit0xc10fdf0 bgfx.cpp1244 Encoder+a8→Draw+68 (drawbaseframe+bc700). Metalrenderer_mtl.mm5184 dispatches setDepthBias:slopeScale:clamp: fromDraw68/6c/70. Current1.26.50.26 FUN14ef6d4d0 __unmapped/0e.cpp2778042–43 hasidenticalGPU→Encoder copy, independentlyprovingcurrenttailtype. CurrentblocktransformFUN140a9df40+callbackFUN144edad40 passresultpointunchangedwithoutgeometryepsilon. Currenttwo-passinsertionbody notindexed; refbehavior transferred withcurrentGPU-state/geometry corroboration.

- Protocol spline wire layout: current artifact6 CameraSplineInstruction schema
  binding FUN_1471fa130; plain curveType type schema doSave0x723f5f0/doLoad0x723d8b0;
  identifier binding0x724f7b0 and load boolean binding0x724ff40. Registry easing is
  optional, inline easing is direct. Local-server fixtures encode those layouts.
- Protocol custom block tags: current FUN_142e87ee0 (`BlockDefinitionGroup::digestServerBlockProperties`)
  reads root `blockTags` list at current __unmapped/02.cpp2462728 and2462987;
  strings are retained, other tag kinds are empty. Constructor tag additions in
  reference DoorBlock/FenceGateBlock/TrapDoorBlock use VanillaBlockTags::OneWayCollidable;
  current catalog string0x106527a8 confirms `one_way_collidable`.
- `sim/world/raycast/camera.rs` shares the existing DDA/corner ordering and shape
  intersection with the authoritative interaction ray; the camera result omits
  provenance collections. Its behavior and allocation comparisons are regression tested.

- Audio listener: reference CameraTraits::isPlayerAudioListener0x020582b0 and CameraRegistry listener presence/value1 gate; current FUN_140a93e80 tests preset offsets0xcd/0xcc and creates PlayerAudioListenerComponent0xe66c48f7. Current predicate FUN_140a9c160 and LevelRendererPlayer::updateListenerState counterpart FUN_144e95380 copy rendered pose when false, player eye/look when true. Shipped camera definitions have no player-listener component, so omitted defaults to camera.
- Target defaults: current FUN_147da1f40 builds CameraTargetSettingsDefinition0x24 bytes, zeros all, writes50f at+8; free camera JSON supplies horizontal[0,360],vertical[0,180]. Current CameraRegistry counterpart FUN_140a93e80 copies the optional horizontal and vertical float pairs directly into target settings.
- Local body visibility: current `ClientInstance::getRenderPlayerModel` FUN_146798550 (current06.cpp1385865 diagnostic) reads CameraRenderPlayerModelComponent from RenderCamera. Callback FUN_1471f5560 (07.cpp322604) copies/removes that marker using CameraBlendState +0x28. The adjacent free-camera pack definition includes camera_render_player_model and extend_player_rendering; first_person lacks the marker. Actor body publication and HUD fallback now follow camera capabilities instead of the saved perspective.

## crates/render-model/src/java_animation.rs (Java Edition 1.7.10, MCP names)
- `java_biped`: ModelBiped.setRotationAngles; `rig_bone`/`rig_from_java_model`: ModelRenderer.render under RendererLivingEntity.doRender's scale(-1,-1,1) and 0.9375 scale.
- `first_person_item`/`first_person_arm`: ItemRenderer.renderItemInFirstPerson and RenderPlayer.renderFirstPersonArm; `draw_item`: ItemRenderer.renderItem, renderItemIn2D, RenderBlocks.renderBlockAsItem.
- `third_person_item`: RenderPlayer.renderEquippedItems; `java_cape_angles`/`java_cape_bone`: its cape block (field_71091_bM chase, cameraYaw, distanceWalkedModified) and ModelBiped.bipedCloak; item classes from ItemSword, ItemTool, ItemHoe, ItemFishingRod, ItemCarrotOnAStick, Item.setFull3D.
- Executed primary witness for `java_animation/reference_tests.rs` and its numeric fixtures: official 1.7.10 client jar at `https://launcher.mojang.com/v1/objects/e80d9b3bf5085002218d4be59e668bac718abbc6/client.jar`, SHA-1 `e80d9b3bf5085002218d4be59e668bac718abbc6`, version metadata `https://piston-meta.mojang.com/v1/packages/ed5d8789ed29872ea2ef1c348302b0c55e3f3468/1.7.10.json`. Scratch harness is outside git in `../java-native-reference/validation/src/{NativeHarness,SliceHarness}.java`.
- `NativeHarness` invokes unchanged `bhm.a(FFFFFFLsa;)V` (ModelBiped.setRotationAngles) reflectively, recording original `bix` pivots/angles for standing, walking, sneaking, riding, blocking, bow use, .37 attack, wrapped look and combined sneak/riding/use states. It then invokes unchanged ModelRenderer render routines in a hidden LWJGL 2.9.1 Pbuffer. Readback/contact sheet stays outside git. Runtime: JRE 8u504, AMD Radeon RX 9060 XT, OpenGL 4.6 compatibility profile `25.10.30.02.250923`.
- `SliceHarness` retains original arithmetic, constants, branches and MathHelper calls from `bop.a(Lblg;F)V`'s cape stack, `bly.a(F)V`'s ordinary and empty-arm stacks and `bly.a(Lsv;Ladd;I)V`'s sprite draw suffix. Snapshot holder fields/query methods replace live player/item lookups; texture binds and draw endpoints become actual GL modelview capture. No arithmetic is rewritten. Numeric `cape.json`, `hand.json` and `arm.json` are column-major GL outputs before Cinnabar frame conversion. This validates fixed-state native transforms, not full-client gameplay, textures or lighting.
- `actor_publication/java/clock_tests.rs` records the actual FSTORE values in the same original `bly.a(F)V` slice for 16 bow and four consumption clock states. Bow uses the unchanged 72000-duration subtraction; consumption uses integer itemInUseCount, then float subtraction of partialTicks and addition of 1. Instrumentation observes the result without rewriting those arithmetic instructions. Private `validation/out/bow-clock.json` records the float bits; no primary artifact or harness is committed.

## crates/client-world/src/actor_animation/java.rs (Java Edition 1.7.10)
- Limb swing: EntityLivingBase.moveEntityWithHeading tail and EntityOtherPlayerMP.onUpdate; hurt flail: handleHealthUpdate(2).
- Hurt event dispatch (`actor_store/hurt.rs` and `actor_animation.rs`): verified official 1.7.10 jar above, `sv.a(B)V` status 2 writes float 1.5 to `sv.aF` immediately, without comparing the hurt countdown. `sv.e(FF)V` copies `aF` to previous `aE`, eases by float 0.4 toward the capped movement target, then adds `aF` to phase `aG`. Consecutive events reset the amount; status 3 alone does not.
- Cape chase: EntityPlayer.onUpdate tail (field_71094_bP/field_71095_bQ/field_71085_bR); bob: EntityOtherPlayerMP.onLivingUpdate and EntityPlayer.onLivingUpdate's grounded/live target; mounted reset: EntityPlayer.updateRidden. Walk distance cast order: Entity.moveEntity. Its walking trigger is disabled by EntityPlayer.canTriggerWalking while PlayerCapabilities.isFlying, freezing walked phase without stopping chasing coordinates. The local predicted flight observation enters through client-presentation/actor_feed.rs, LocalPlayerFeed and ActorTickContext.
- Swing: EntityLivingBase.updateArmSwingProgress and swingItem. Walk accumulation and cast order: Entity.moveEntity, with EntityPlayer.canTriggerWalking.
- Body yaw: EntityLivingBase.onUpdate and func_110146_f; equip: ItemRenderer.updateEquippedItem with Minecraft.rightClickMouse's resetEquippedProgress2.

## crates/client-presentation/src/actor_publication/java/mounted.rs (Java Edition 1.7.10)
- RendererLivingEntity.doRender's EntityLivingBase ridingEntity branch interpolates the mount's renderYawOffset, wraps and clamps head lag to ±85°, and pulls the displayed body by a fifth beyond 50°. RenderPlayer.renderEquippedItems cape code independently reads the player's original renderYawOffset.
- Fixed body/head/pitch/relative-angle outputs in java/fixtures/mounted.json were executed from the official RendererLivingEntity bytecode with field owners rebound to fixed snapshots; arithmetic and branches remained unchanged. ActorSnapshot's mount species predicate intentionally recognizes only players and known built-in rideable living species, rather than inferring EntityLivingBase from optional health attributes.

## crates/client-presentation/src/camera/java.rs (Java Edition 1.7.10)
- EntityRenderer.setupViewBobbing and hurtCameraEffect; EntityPlayer.onLivingUpdate cameraYaw/cameraPitch (health gates and float/double cast order); EntityPlayer.updateRidden; Entity.moveEntity walked-distance cast order; EntityPlayerSP renderArmPitch/renderArmYaw.
- `camera/java/reference_tests.rs` numeric fixtures execute the same primary jar's `blt.g(F)V` bob, `blt.f(F)V` live hurt stack with zero unavailable attack direction, and `bly.a(F)V` arm sway slice through `SliceHarness`. Only snapshot queries and renderer endpoints are substituted; actual OpenGL stacks are captured. Idle/walking/airborne bob, middle/end hurt and positive/negative hand sway states are covered.
- Mounted yaw fixtures in `actor_publication/java/fixtures` execute the original `boh.a(Lsv;DDDFF)V` body/head interpolation and living-mount clamp block, including its unchanged private interpolation helper bytecode and `qh.g(F)F` wrap. Recorded cases exercise wrapped interpolation and both ±85° clamp extremes (resulting ±68° head/body offset).
- Java camera sneak height: EntityPlayerSP.onLivingUpdate yOffset2 and Entity.moveEntity decay; death: EntityRenderer.hurtCameraEffect and RendererLivingEntity.rotateCorpse.

## crates/render/src/hand_rig.wgsl and hand_rig_render.rs (Java Edition 1.7.10)
- ItemRenderer.renderItemInFirstPerson calls RenderHelper.enableStandardItemLighting after pitch/yaw rotation, before arm sway. RenderHelper uses normalized (0.2F,1,-0.7F) and (-0.2F,1,0.7F), diffuse 0.6, global ambient 0.4, zero specular, and GL_FLAT. ItemRenderer enables GL_RESCALE_NORMAL before held-item and empty-arm drawing. The first-person pass retains the world lightmap multiplied into gamma RGB.
- Primary source jar witness is the official 1.7.10 client indexed above; local inspection paths are scratchpad/je1710/src/net/minecraft/client/renderer/{ItemRenderer,RenderHelper}.java. No source files or assets are committed.
- OpenGL 2.1 specification §2.11.3 specifies inverse-transpose normal transformation and RESCALE_NORMAL factor 1/sqrt(m31²+m32²+m33²), where mij are the modelview inverse: https://registry.khronos.org/OpenGL/specs/gl/glspec21.pdf . The raster-depth normal maps that native Z row into the attachable rig frame before shader rescaling.
## Worn elytra rendering

- `crates/pack-compiler/src/entity/legacy_block_geometry.rs`: the pinned pack's
  `models/mobs.json` owns `geometry.elytra`; `attachables/elytra.json` selects it.
- `crates/client-presentation/src/presentation/equipment/runtime/elytra.rs`:
  `animations/elytra.animation.json` and
  `animation_controllers/elytra.animation_controllers.json` own wing poses,
  descending-movement spread and shortest-path transition blending.
- `crates/client-presentation/src/presentation/cape.rs`: humanoid additional
  rendering's chest-gear path uses the player's cape raster for worn elytra.
- `crates/render/src/actor/glint.rs`: ActorShaderManager foil parameters define
  the 1375/3750-ms scroll periods, -20/80-degree rotations and RGB multiplier.
- `crates/render/src/actor.wgsl`: independent fragment/vertex evidence from
  `~/coding/go/lunar/minecraft-apk/split_install_pack.apk`,
  `assets/assets/renderer/materials/ActorGlint.material.bin`; its base APK manifest
  identifies 1.26.31.1. UV rotation uses the texture center; summed glint samples
  are multiplied by glint color and tile light before RGB is squared and added
  to the shaded base before fog. This is an older shader cross-check, not a
  version-matched 1.26.50 pixel acceptance witness.
- Current foil uniform evidence: `ActorShaderManager::setupFoilShaderParameters`
  (`R:ActorShaderManager:1250`, `R:ActorShaderManager:1436`) and constants
  `0x10dd0cff0`, `0x10dd0d5a0`, `0x10dd0d000`; cape-image selection:
  `R:DataDrivenRenderer_tempComponent_HumanoidAdditionalRendering:3687`.
- `crates/client-world/src/actor_animation/tick.rs`: controller transitions reset
  the blend timer and replace the outgoing state with the immediately preceding
  current state (`R:ActorAnimationControllerPlayer:912`). During a blend, both
  state players are resampled with the current render queries; shortest-path
  blending combines their sampled bone maps (`R:ActorAnimationControllerPlayer:1093`,
  `R:ActorAnimationControllerPlayer:1327`). Interrupted blends therefore restart
  from that outgoing state's clip rather than a snapshot of the previously
  blended pose.
- `crates/client-world/src/actor_animation/pose.rs`: shortest-path blends sample
  each state into a fresh bone map, lerp them, then add translation/rotation and
  multiply scale into the accumulated map (`ActorAnimationControllerPlayer::blendViaShortestPath`,
  `R:ActorAnimationControllerPlayer:2395`); other blends apply both state players
  onto the shared map with weights `w` and `1 - w` (`R:ActorAnimationControllerPlayer:1158`).
  The blend timer resets on transition and accumulates each frame's delta
  (`R:ActorAnimationControllerPlayer:1098`), so it starts at the transition's frame fraction.

## Primitive shapes: protocol, state and reference rules

Files: `crates/protocol/src/primitive_shapes.rs`,
`crates/render-api/src/primitive_shapes.rs`,
`crates/render-model/src/primitive_shapes/`, `docs/reference/primitive-shapes.md`.

Current Lens function reads corroborated the older owner-organized lookup files under
`~/coding/go/lunar/refs/mcsrc-1.26.50/reference/26.30/src/by-owner/`; the directory label alone
was not used as version evidence.

- `ClientNetworkHandler::handle(PrimitiveShapesPacket)` `0x103541c60`: client dispatch.
- `ClientScriptPrimitiveShapesDataComponent::handlePacket` `0x1022bab10`: ordered id lookup,
  absent-type removal, present-type creation/update, no explicit count cap.
- `PrimitiveShapeDataPayload::constructShape` `0x1065c93e0`: kinds and creation defaults.
- `ScriptPrimitiveShape::applyUpdatedData` `0x10918c750`: optional patches, zero lifetime,
  negative-distance reset, dimension and actor unique id.
- `ScriptSpherePrimitive::applyUpdatedData` `0x109191530` and
  `ScriptCirclePrimitive::applyUpdatedData` `0x109192b50`: byte segment count.
- `ScriptArrowPrimitive` constructor `0x109195740` and updater `0x109195af0`:
  independent optional head and endpoint fields; `0x10d986350` f32 pair `[0.5, 1.0]` gives
  default radius and length.
- `ScriptTextPrimitive` constructor `0x109192c20` and updater `0x109193240`: default options,
  text-object parsing, complete text option replacement and background clearing.
- `ClientScriptPrimitiveShapesSystem::tick` `0x1022bc960`: dimension filtering, missing-actor
  suppression and render-helper construction; no local lifetime decrement.
- Server primitive system tick adapter `0x10538dc30`: monotonic elapsed-time subtraction and
  removal at remaining lifetime `<= 0`.
- `serialize<mce::Color>::read` `0x1066bf090`, `cerealizer<mce::Color>::bind` `0x106e63a70`:
  ARGB channel order in the four-byte wire integer.
- `Scripting::RenderHelper::Renderer::convertStringsToNameTags` `0x104449a80`: literal
  backslash-n replacement, discard-empty line splitting and integer half widths.

## Primitive shape rendering

- `crates/render/src/primitive_shapes/mesh.rs`: current Lens `Scripting::RenderHelper::LinePrimitive::_rebuild` at `0x10443dab0`, `BoxPrimitive::_rebuild` at `0x10443d540`, `DiscPrimitive::_rebuild` at `0x10443dce0`, `AxialSpherePrimitive::_rebuild` at `0x10443df80`, `ArrowPrimitive::_rebuild` at `0x10443e290`, and `generateDiscVerts` at `0x10443ed90`.
- `crates/render/src/primitive_shapes/pipeline.rs`: current Lens `Scripting::RenderHelper::Renderer::onEndRender` at `0x104447e40` submits line-list vertices through the `debug` material. Installed PlayCover `data/resource_packs/vanilla/materials/ui3D.material` lines 454–466 corroborate LessEqual, default depth write and no blending; the installed asset version is 1.26.51.01, not a matched 1.26.50 witness.
- `crates/render/src/primitive_shapes/shapes.wgsl`: `BasePrimitive::getAttachedToPosition` uses interpolated riding position; `Renderer::convertStringsToNameTags` at `0x104449a80` forwards text to `BaseActorRenderer::extractRenderTextObjects` at `0x103dc03c0` and `_extractRenderTextObject` at `0x103dc0840`. `LevelNameTagRenderer::renderText` at `0x103eb9050` applies incoming scale times 1.6 times 1/60 and fixed 0.125-per-extra-line lift; data reads at `0x10d9a15a0`, `0x10db88db4`, `0x10d91dae0` confirmed these constants.
- Local files consulted are the matching owner files under `~/coding/go/lunar/refs/mcsrc-1.26.50/reference/26.30/src/by-owner`; their catalog is labeled 26.30. Current function reads corroborate `Renderer::onEndRender` and `convertStringsToNameTags`; full material/geometry capture parity remains open.

- `crates/render-model/src/primitive_shapes/state.rs`: current `ClientScriptPrimitiveShapesSystem::tick` `0x1022bc960` text quaternion is `Rz * Ry * Rx`; data at `0x10d972210` and `0x10d8ec9d8` are degree-to-radian and half-angle constants.
- `crates/render/src/primitive_shapes/shapes.wgsl`: current `Renderer::onBeginRender` `0x104446fa0` performs strict squared-distance comparison against the optional shape range or context range. `LevelRenderer::renderLevel` `0x10436ad94` passes the range computed by `LevelRendererPlayer::recalculateRenderDistance` `0x10439a330`, already mirrored by the cloud distance helper. Vtable reads at `0x110c96290` and `0x110c96360` select `BasePrimitivePosition::getPosition` `0x10443d500`; box tick construction supplies its lower corner.
- `crates/client-presentation/src/primitive_shapes.rs`: `BasePrimitive::getAttachedToPosition` `0x10443d390` gets interpolated riding position and subtracts `OffsetsComponent` vertical offset; existing actor-store snapshots already normalize feet positions and seat riders before interpolation.

- Primitive draw ordering: `Renderer::onBeginRender` comparator in `__sort3` `0x104479810` compares signed `BasePrimitive + 0xc`; packet helper construction in `ClientScriptPrimitiveShapesSystem::tick` `0x1022bc960` initializes that priority to zero for all supported geometry. The unstable introsort does not order by kind or distance.

- `crates/client-ui/src/ui_runtime/presentation/primitive_shapes.rs`: current `ScriptTextPrimitive::applyUpdatedData` `0x109193240` retains parsed `TextObjectRoot` or literal; `Renderer::onBeginRender` `0x104446fa0` resolves only when the helper dirty flag or player input/interaction mode changes. Domain dynamic-text markers preserve common-patch refresh without rebuilding literal text geometry.

- Equal packet updates: current `ClientScriptPrimitiveShapesDataComponent::handlePacket` `0x1022bab10` unconditionally marks an existing present-type entry dirty after its updater, with no equality check. `generateDiscVerts` `0x10443ed90` zero-segment branch initializes both closing vertices and packed colors to zero, then appends the closing pair unconditionally.

## Translation parameter localization
- `Localization::_get` localizes a parameter only when it begins with `%`, using the whole remaining parameter as a key; unresolved keys retain the original argument. Ordinary player names and embedded percent text are literal. R:Localization:1830-1940.

## crates/client-ui/src/ui_runtime/presentation/gui_models/held.rs
- Current 1.26.50.26 banner held path: humanoid additional rendering `0x05e2b300` calls banner item rendering `0x06c592a0`, sharing setup `0x06c57b00` with GUI `0x06c581a0`. The held renderer draws pole, crossbar and cloth with base/pattern materials. The existing sprite fallback preserves availability only; exact held geometry remains an open parity item.

## Crosshair presentation preferences

- `crates/client-ui/src/ui_runtime/presentation/forms/engine/hud_renderers.rs`:
  current 1.26.50.26 `FUN_149c5ff60` (artifact 6, RVA `0x9c5ff60`) selects
  `ui_crosshair` and `textures/ui/cross_hair`, centered at 16×16 GUI pixels.
  Vanilla 1.26.50.04 `materials/ui.material` makes `ui_crosshair` inherit
  `ui_invert_overlay`, using `OneMinusDestColor` and `OneMinusSrcColor`.
- `crates/client-ui/src/ui_runtime/presentation/hud_layout/status_rows.rs`:
  26.30 `HudCursorRenderer::render` (`0x1020b3700`) returns when
  `ClientInstance::getRenderPlayerModel` (`0x102342080`) is true; the
  `ClientInstance` vtable at `0x110b28730`, slot `+0x6a0`, confirms the call.
  Current getter `FUN_146798550` (`0x6798550`) corroborates the camera's
  `CameraRenderPlayerModelComponent` test, with an editor exception.
  The current renderer's virtual slot has not been independently mapped.
- Third-person visibility and disabling inversion are owner-requested options;
  defaults retain first-person visibility and inverted colors. The Java HUD's
  built-in fallback remains 15×15; a pack crosshair remains 16×16.

## Swing duration publication

- `crates/gameplay/src/melee.rs` and `melee/swing.rs`: Bedrock 1.26.50 `Mob::getModifiedSwingDuration`, `Mob::swing` and `Mob::aiStep`; Java 1.7.10 `EntityLivingBase.getArmSwingAnimationEnd`, `swingItem` and `updateArmSwingProgress`.
- `crates/client-world/src/actor_animation/motion.rs` and `tick.rs`: Bedrock 1.26.50 `Mob::aiStep` and `Mob::swing`; Java swing publication follows `EntityLivingBase.updateArmSwingProgress`.
- `crates/client-world/src/actor_animation/render_frame.rs`, `render_frame/clips.rs` and `tick/selection.rs`: vanilla pack `animation_controllers/player.animation_controllers.json` first-person attack weights and `animations/player.animation.json` attack channels.
- `app/src/runtime/network/actor_publication.rs` and `crates/client-presentation/src/actor_publication/preparation.rs`: local tick admission precedes swing-counter publication; actor picking retains the actor interpolation boundary.
- `crates/client-world/src/actor_animation/java/body.rs` and `local_motion.rs`: Java 1.7.10 `EntityLivingBase.onUpdate` calls `onLivingUpdate` before its swing-dependent facing choice and `func_110146_f`; `EntityPlayer.updateEntityActionState` updates arm swing progress during that living update.
- Rules: `docs/reference/swing-duration.md`.

## Desktop cursor focus ownership

- `crates/client-presentation/src/camera/focus.rs`, `app/src/camera/focus.rs`, `app/src/camera/focus/native.rs`: `MinecraftGame::onAppFocusLost` releases held controls and cursor capture; `onAppFocusGained` checks the active screen before capture (R:MinecraftGame:102683–103207).
- Focus-loss pause preference: R:GeneralSettingsFactoryAnon--b69a8d87dfcb:1789. Gameplay steals mouse outside touch input; ordinary screens do not (R:InGamePlayScreen:4997–5062; R:BaseScreen:823–830).
- Cursor release clears logical capture and shows the pointer (R:MinecraftGame:124942–125235). macOS periodic centering requires captured state (R:__unmapped/00:1690783–1690824).

- Version-matched Windows focus handlers: `current/1.26.50.26/src/__unmapped/00.cpp`: loss 1606419–1606753, gain 1606898–1607108; pause-screen dispatch 825180–825251 and 876546 onward; factory constructs `pause.pause_screen` at 1021123. Windows capture/release use hide/show, clip/unclip, and capture/release at 152158–152249. Focus-pause option is mapped in `__unmapped/04.cpp`:1635569–1635571.

## Held block placement (1.26.50)

Files: `docs/reference/held-block-placement.md`, `crates/gameplay/src/block_use.rs`,
`crates/gameplay/src/block_use/intention.rs`, `crates/gameplay/src/block_use/packets.rs`,
`crates/gameplay/src/block_use/stopping.rs`, `app/src/block_use.rs`,
`app/src/block_use/target.rs`, `crates/protocol/src/interaction.rs`.

- Primary current evidence: Lens artifact 6, normalized build `1.26.50.26`,
  source-backed `artifact_function` reads at RVAs `0x28b63a0`
  (`continueBuildBlockAction`), `0x28b66b0` (`continueBuildBlock`),
  `0x28b6f00` (`stopBuildBlock`), and `0x28d3ed0` (build callback).
  Local canonical counterparts are
  `~/coding/go/lunar/refs/mcsrc-1.26.50/current/1.26.50.26/src/__unmapped/02.cpp`.
  These confirm the hold flags, fresh-pick targeting, strict due-time comparison,
  movement direction, line-cell intersection, cached orientation intercept,
  clicked runtime ID before prediction, start/swing/transaction order and miss click zero.
- Current Lens artifact 6, `_tickBuildAction` at `0x67883a0`, refreshes picks before
  held continuation. The target-type early returns in `0x28b63a0` retain hold flags;
  `useItemOn` at `0x28b74d0` disables intention after interaction while retaining the line.
- Lens `read_data` on artifact 6 confirms float data addresses:
  `0x150132700` = 300 ms, `0x14ff9cdc8` = 200 ms,
  `0x15005ea40` = 900 speed divisor, `0x14feff2c8` = 180 ms,
  `0x14ffa2648` = 100 ms survival floor, `0x14ffa90dc` = 20 Hz movement scale,
  `0x1500c2b70` = 0.01 squared movement threshold.
- Named-owner reference root:
  `~/coding/go/lunar/refs/mcsrc-1.26.50/reference/26.30/src`.
  `by-owner/g/GameMode.cpp`: `startBuildBlock` RVA `0xa0bd770`,
  `buildBlock` `0xa0bd790`, `continueBuildBlockAction` `0xa0bdd30`,
  `continueBuildBlock` `0xa0bdfe0`, `_calculatePlacePos` `0xa0be670`,
  `stopBuildBlock` `0xa0be750`, and `getPickRange` `0xa0c1450`.
  The GameMode hold stores prior success, interactive-use and placement-intention flags,
  successful destination, locked direction/face, next destination and first world intercept.
- `by-owner/c/ClientInstance.cpp`: `_tickBuildAction` RVA `0x2334cd0`,
  refreshes main and liquid HitResults before held continuation; `resetBai`,
  `clearInProgressBAI` and `getInProgressBAI` own input intention.
  `by-owner/h/HitResultUtils.cpp`: `refreshHitResult` refreshes world pick evidence.
  `by-owner/c/ClientInputCallbacks.cpp`: `handleBuildAction` RVA `0x23178c0`
  owns first press; selected-slot and gameplay-input routing own stopping boundaries.
- `by-owner/b/BlockItem.cpp`: `_calculatePlacePos` RVA `0xa5f6dc0`,
  replace the clicked cell when admitted, otherwise offset by face.
  `by-owner/p/PlanterItemComponent.cpp`: `getBlockPlacementContext` RVA `0xa2e0430`
  supplies the qualifying intention bit, which is not equivalent to any nonzero block ID.
  Current Lens artifact 6, source-backed raw RVA `0x3bd0ef0`, confirms
  property mask `0x40003`, three virtual bool predicates, and carpet bit `0x80`
  at BlockType offset `0x130`; custom block_placer component offset `0x42` enables
  intention. The named reference uses mask `0x200003` and carpet bit `0x100` at
  offset `0x108`. Lens vtable pointer read at `0x110ff1998` maps slots
  `0xe8/0xf8/0x100` to `isFenceBlock/isThinFenceBlock/isWallBlock`;
  `isFenceGateBlock` at `0xf0` is omitted. Stair and slab constructors set bits
  1 and 2; the ordinary BlockType constructor supplies cube `0x200000`.
  Runtime block-property admission in Cinnabar remains incomplete.
  `crates/gameplay/src/movement/collision_registries.rs` and its `tags.rs`
  admit conservative model families; `by-owner/l/LeavesBlock.cpp:130–145`
  ORs leaf properties into the inherited ordinary cube properties, confirming
  leaves retain placement intention. Chest, Bed, FenceGate and TrapDoor constructors
  replace the default property mask and do not qualify merely from collision shape.
  `by-owner/s/SoulSandBlock.cpp:38`, `by-owner/m/MudBlock.cpp:95`,
  `by-owner/b/BarrierBlock.cpp:38`, and `by-owner/c/ChiseledBookShelfBlock.cpp:146`
  call the ordinary BlockType constructor without replacing its cube property;
  the identifier exceptions and their two-ID-space regressions cover these cases.
- `by-owner/b/BlockStateHelper.cpp`: `isAnyDirection` RVA `0xb4b8560`,
  initial world-intercept caching for cardinal direction, facing direction, vertical half,
  direction, weirdo direction, orientation, upside-down/top-slot, standing rotation,
  hanging, rotation, torch facing, vine bits and coral direction; pillar axis is absent.
- Packet reference owners: `support/std/__func--bc8d9908b204/9.cpp:130567–131030`
  (build callback), `support/std/__func--bc8d9908b204/1.cpp:135810–135897`
  (use callback), `by-owner/c/CommonGameModeMessenger.cpp:101–229`,
  `by-owner/i/ItemUseInventoryTransaction.cpp:3529–3556`,
  `by-owner/l/LocalPlayer.cpp:10292–10413`. Lens named function
  `0x10a0dc360` corroborates transaction construction. The current callback above
  corroborates the first-success action and transaction fields for the target build.

- Current Lens artifact 6, `handleItemStackResponse` at `0x28d10d0`, rejects IDs
  unless `(~request_id & 0x80000001) == 0`: only negative odd client request IDs.
  Canonical current `02.cpp:1476783–1476901` confirms this validation. Legacy
  even placement scopes do not register in that response queue.
  The local server pin `hashimthearab/dragonfly@58003c1d2ced`,
  `server/session/handler_inventory_transaction.go:18–74`, processes
  `LegacySetItemSlots` through `sendItem`/`sendInv`; `server/session/player.go:258–277`
  sends `InventorySlot`/`InventoryContent`, not `ItemStackResponse`.

## Absorption hearts

- `crates/client-ui/src/ui_runtime/hud_adapter.rs`, `crates/ui/src/hud.rs`, and
  `crates/client-ui/src/ui_runtime/presentation/hud_layout/status_rows.rs`:
  1.26.50.26 artifact 6 `FUN_149c64c10` (RVA `0x9c64c10`) reads absorption
  current independently of its maximum, rounds upward, appends after health,
  wraps at ten with fixed 10-pixel rows, blinks all container backgrounds, and selects wither sprites for absorption
  only when wither wins the effect precedence. `FUN_149c65980` (RVA `0x9c65980`)
  loads absorption full/half textures and hardcore variants.
- `crates/json-ui/src/hud/tests.rs`: vanilla pack 1.26.50.4
  `resource_pack/ui/hud_screen.json`, `heart_renderer`, binds only
  `#show_survival_ui` to `#visible`; absorption is native renderer state.
