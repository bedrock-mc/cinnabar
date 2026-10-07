# Java 1.7 animations

Owner-mandated default, selected with Video › Animations: Java 1.7 or Bedrock (live, persisted). The selector uses the same dropdown and radio controls as Graphics mode, with pointer, keyboard and controller access. Selecting Bedrock keeps every vanilla path.

New installs and existing installs with no saved animation preference default to Java 1.7. Saved toggle values migrate: on selects Java 1.7; off selects Bedrock.

Code: `render-model::java_animation` (pose, cape and matrix stacks), `client-world` `actor_animation/java.rs` (tick motion, retargeting), `client-presentation` `actor_publication/java.rs`, `camera/java.rs` and `equipment/runtime/java.rs`.

## Frame mapping

Every Java stack is evaluated in Java's own frame and carried into ours by one fixed conversion per frame pair; no pose has its own correction.

| Java frame | Our frame | Conversion |
| --- | --- | --- |
| Hand pass eye space: blocks, +X right, +Y up, -Z ahead, 70° vertical FOV (never the FOV setting) | Hand pass camera space, 70° vertical | Identity |
| Model space: pixels, Y down, origin at the neck 24 px above the feet, drawn under `scale(-1,-1,1)` | Rig frame: blocks, Y up from the feet, X mirrored | `rig = T(0, 24/16, 0) · S(-1,-1,1) · java / 16` |
| Part angles, applied Z·Y·X about the rotation point | Bone rotation in the rig frame | `S·Rz(z)Ry(y)Rx(x)·S = Rz(z)·Ry(-y)·Rx(-x)` with `S = S(-1,-1,1)` |
| Rotation point | Bone translation (pixels) | rig rest pivot `+ S·(point - rest point)`, so slim or custom pivots keep their offsets |
| World: `T(pos)·Ry(180-yaw)·S(-1,-1,1)·S(0.9375)·T(0,-1.5078125)` | `rig_world_from_actor(pos, yaw, 0.9375)` | Ours `· T(0, 1/128, 0)` (Java's 1/128 lift) |
| Arm `postRender` frame for held items | Right-arm bone frame | `S(-1,-1,1)`, then less the hand bone's rest offset from the arm |
| Flat item slab: x = 1 - u, y = 1 - v, z in [-1/16, 0] | Held sprite slab, x = -u | `T(1, 0, 0)` |
| Item cube: quarter turn, then `T(-0.5)` on the unit cube | Centred unit cube | `Ry(90°)` |
| Flat item slab | Raster attachable (bow pull frames): image column, depth, row | `(c, d, r) → (1 - c/w, 1 - r/h, -d/16)` |

The empty hand's camera matrix is Java's stack times `rig⁻¹`, with the arm bone at Java's rest pose (rotation point (-5, 2, 0), angles (0, 0, 0.1 rad)). Golden tests project Java's arm-box corners and item texels through Java's own GL calls (an f64 stack emulator) and through our rig, bone and mesh, and assert the same screen pixels at 854×480.

## Java 1.7 animation rules

Trig uses Java's 65536-entry sine table (angle × 10430.378, truncated, masked). Matrix calls read in call order, each post-multiplying. `s` is swing progress, `e` equip progress, both interpolated to the frame.

| Rule | Behaviour |
| --- | --- |
| Swing timing | 6 ticks, recalculated each tick; Haste `6 - (1 + amp)`, Mining Fatigue `6 + 2(1 + amp)`; restarts only past its first half; the frame value wraps forward from 5/6 to 1. |
| Equip | Moves 0.4 a tick toward 1 (same item and data value; count and unrelated tags never re-equip; durability is a data value; a stable stack keeps its height when that value changes in place) or 0; the new item is adopted below 0.1, and the old item stays drawn until then, in vanilla's hand for an item only vanilla draws. The selected item's use clock never animates the retained outgoing item. An identical stack in another slot delays the dip by one tick; empty slots do not dip. A block placement drops it to 0 before that tick's rise; starting a use does not. |
| Use counts | Java's integer in-use count is `duration - (use_ticks - 1)` for our consecutive using ticks. Eat/drink converts that count to float, subtracts the frame fraction, then adds 1. Bow subtracts that result from its 72,000-tick duration. Keeping that order preserves the original rounding, including its quantized bow frame fractions. |
| First-person prefix | Eat/drink raise, or (not in use) `T(-0.4·sin(√s·π), 0.2·sin(2√s·π), -0.2·sin(s·π))`; then `T(0.56, -0.52 - 0.6(1 - e), -0.72)`, `Ry(45)`, `Ry(-20·sin(s²π))`, `Rz(-20·sin(√s·π))`, `Rx(-80·sin(√s·π))`, `S(0.4)`. In use there is no swing translate, so block-hitting keeps only the swing turns. |
| Eat/drink raise | `t` as above, `r = 1 - t/duration`, `k = 1 - (1 - r)^27`: `T(0, |0.1·cos(t/4·π)|` when `r > 0.2`, `0)`, `T(0.6k, -0.5k, 0)`, `Ry(90k)`, `Rx(10k)`, `Rz(30k)`. |
| Sword block | After the scale: `T(-0.5, 0.2, 0)`, `Ry(30)`, `Rx(-80)`, `Ry(60)`. |
| Bow | `Rz(-18)`, `Ry(-12)`, `Rx(-8)`, `T(-0.9, 0.2, 0)`; `d = min((p²/400 + p/10)/3, 1)`; shake `T(0, 0.01·sin(1.3(p - 0.1))·(d - 0.1), 0)` past `d = 0.1`; `T(0, 0, 0.1d)`, `Rz(-335)`, `Ry(-50)`, `T(0, 0.5, 0)`, `S(1, 1, 1 + 0.2d)`, `T(0, -0.5, 0)`, `Ry(50)`, `Rz(335)`. Pull frames: standby, then frames 0/1/2 after more than 0, 13 and from 18 whole draw ticks. |
| Rods | Fishing rods and on-a-stick items turn `Ry(180)` before drawing. |
| Flat item draw | `T(0, -0.3, 0)`, `S(1.5)`, `Ry(50)`, `Rz(335)`, `T(-0.9375, -0.0625, 0)` onto the unit slab. Cubes take only the quarter turn. |
| Empty hand | `T(-0.3·sin(√s·π), 0.4·sin(2√s·π), -0.4·sin(s·π))`, `T(0.64, -0.6 - 0.6(1 - e), -0.72)`, `Ry(45)`, `Ry(70·sin(√s·π))`, `Rz(-20·sin(s²π))`, `T(-1, 3.6, 3.5)`, `Rz(120)`, `Rx(200)`, `Ry(-135)`, `T(5.6, 0, 0)`. Ordinary held items hide the arm; maps are the two-arm exception. |
| First-person lighting | Ambient 0.4 plus two diffuse 0.6 lights along normalized `(0.2, 1, -0.7)` and `(-0.2, 1, 0.7)`. Camera hurt/bob and look rotate the lights before hand sway. Normals use inverse transpose and legacy rescaling by the inverse native Z-row length, including bow stretch. Texture, directional shade and byte-quantized world light multiply in gamma space. |
| Hand motion | Hurt roll, then view bob, then sway `Rx(0.1·(pitch - armPitch))`, `Ry(0.1·(yaw - armYaw))`; the arm angles move halfway to the look each tick and sway is always on. |
| View bob | Walk phase `w = -(d + (d - d_prev)·frame)` (one tick ahead); `T(0.5·sin(wπ)·b, -|cos(wπ)·b|, 0)`, `Rz(3·sin(wπ)·b)`, `Rx(5·|cos(wπ - 0.2)·b|)`, `Rx(fall)`. `d += 0.6` per block walked (not flying, riding or sneaking on the ground); `b` eases 40% a tick to the capped 0.1 horizontal speed on the ground; `fall` eases 80% to `15·atan(-0.2·vy)` in the air. Death sets both targets to zero. |
| Death roll | `Rz(40 - 8000/(deathTicks + frame + 200))` before hurt roll. The body tips by `min(sqrt((deathTicks + frame - 1)/20 · 1.6), 1) · 90°`. |
| Sneak camera | Held sneak lowers the rendered eye by 0.08 blocks; release decays the drop by 0.4 per tick, interpolated to the frame. Gameplay keeps the Bedrock eye origin. |
| Hurt roll | `Rz(-14·sin(h⁴π))`, `h` the hurt time left over 10 ticks; never a direction. |
| Limbs | Amount eases 40% a tick to `min(4·step, 1)` and the swing accumulates it; each hurt event immediately sets the amount to 1.5, including consecutive hits. Death alone does not apply the boost. The frame reads `swing - amount·(1 - frame)` and the clamped interpolated amount. |
| Body yaw | Turns 30% toward the walk direction (moving more than 0.05 blocks a tick) or the look while swinging; head lag clamps to ±75, and past 50 degrees the body is pulled a fifth of the lag back. Local head look reads the current render-frame input against that interpolated body heading; remote look remains interpolated between ticks. |
| Pose | Arms `X = cos(0.6662·limb (+π right))·amount`, legs `1.4·` the opposite; riding `-36°` arms, `-72°`/`±18°` legs; holding `X·0.5 - 18°·n` (n = 1, 3 blocking); attack `body Y = 0.2·sin(2π√s)` with the arm points circling the body and the right arm lifted by `1.2·sin(π(1 - (1 - s)⁴)) - 0.75·sin(sπ)·(head X - 0.7)`, `Z = -0.4·sin(sπ)`; sneak body 0.5 rad, arms +0.4, legs at (y 9, z 4), head y 1 (otherwise legs z 0.1); idle `Z ±= 0.05·cos(0.09·age) + 0.05`, `X ±= 0.05·sin(0.067·age)`; bow aim arms `X = -90° + head X`, `Y = -0.1/+0.5 + head Y`. Parts are separate: the head and arms do not follow the body. |
| Third-person grips | After `T(-1/16, 7/16, 1/16)`: cube `T(0, 0.1875, -0.3125)`, `Rx(20)`, `Ry(45)`, `S(-0.375, -0.375, 0.375)`; bow `T(0, 0.125, 0.3125)`, `Ry(-20)`, `S(0.625, -0.625, 0.625)`, `Rx(-100)`, `Ry(45)`; tools (swords, pickaxes, axes, shovels, hoes, sticks, bones, rods) rods first `Rz(180)`, `T(0, -0.125, 0)`, blocking `T(0.05, 0, -0.1)`, `Ry(-50)`, `Rx(-10)`, `Rz(-60)`, then `T(0, 0.1875, 0)`, `S(0.625, -0.625, 0.625)`, `Rx(-100)`, `Ry(45)`; other items `T(0.25, 0.1875, -0.1875)`, `S(0.375)`, `Rz(60)`, `Rx(-90)`, `Rz(20)`. |
| Placement | Sneaking players draw 0.125 lower (the local player 0.08, its eased step offset). The red hurt flash covers body and armour but not held items. |
| Cape | A chasing point closes a quarter of its lag to the position each tick (an axis more than 10 blocks off snaps, still adding that quarter). From its lag `d` and body yaw `y`: `back = max(100·(d.x·sin y - d.z·cos y), 0)`, `side = 100·(-d.x·cos y - d.z·sin y)`, `lift = clamp(10·d.y, -6, 32) + 32·sin(6·walked)·b`, plus 25 sneaking; then `T(0, 0, 2 px)`, `Rx(6 + back/2 + lift)`, `Rz(side/2)`, `Ry(-side/2)`, `Ry(180)` in model space, so the body's tilt and swing never move it. Only the local player has a walked distance, which holds while flying, riding or sneaking on the ground; 1.7 clamps nothing else and hides the cape only when invisible. |
| Blocking trigger | A sword with the using flag; locally, holding use with a sword, since Bedrock never flags that use. |

## Kept vanilla or left out

- Swimming, crawling, gliding, sleeping and emoting keep vanilla poses: Java 1.7 has none.
- Server-authored player rigs and first-person attachables retain their own poses. Uploaded custom/slim skins remain eligible; local emotes drive their animated skin layers with the same sampled pose as the body.
- Maps, crossbows, tridents, shields, spyglasses and other held attachables keep vanilla's first-person hand; off-hand items keep vanilla placement on Java's arm.
- Elytra keeps its authored wing poses in both animation modes and hides the separate cape while worn. When a cape is present, its texture replaces the wing texture. The third-person bow pull frames and the cast rod drawn as a stick are not Java's.
- First-person directional lighting follows Java; environment brightness still comes from the shared Bedrock lightmap. Exact Java lightmap colors, tinted multipass saturation and complete frame lighting remain outside the verified scope.
- Java's first-person arm can inherit another player's riding pose through a shared model; that bug is not reproduced. Skins keep their outer layers and slim arms (Java 1.7 had neither).
- Recognized living mounts supply the displayed player's body heading. Head lag clamps to ±85°; above 50° the body moves another fifth toward the head, reducing extreme relative head output to ±68°. Mount yaw takes the short interpolation path. The cape retains the player's ordinary body yaw. Nonliving vehicles and unclassified mount identities keep the ordinary player body basis.
- Active local creative flight freezes the cape's walking phase while its trailing motion and limbs continue. The phase resumes after landing; permission to fly alone does not freeze it.
- No hurt particles: Java 1.7 has none tied to the animation.

## Reference validation

Fixed Java 1.7.10 state fixtures cover biped pivots and angles, cape corners, first-person
item and empty-arm matrices, camera bob/hurt/sway, and mounted yaw limits. There are 42
pose/transform states and 20 exact use-clock states. Native model
readbacks and Cinnabar Windows/DX12 captures check the visible pose and geometry. Numeric
comparisons allow floating-point roundoff; these checks do not assert matching lighting,
modern item behavior, custom mount classification, or full game-frame pixel equality.

The original combined PR revision passed 802 tests across client-world, client-presentation and
render-model, with 12 existing ignored cases. Touched-crate checks and the optimized
developer-control client build pass using sccache. Windows/DX12 hidden captures exercise
flight, landing, horse/boat mounting, yaw wraparound, dismounting, first-person use and
held-item swaps, both third-person views, and local emote coexistence. These are animation
and geometry witnesses, not a performance benchmark or full-client pixel comparison.

`java_hand_lights_normals_and_composes_texture_in_gamma` renders the production hand shader
on a GPU. Original test texels cover arms and item layers, ambient and directional faces,
world brightness, and nonuniform bow stretch in a rotated raster frame. The publication
light test checks world look and camera effects while excluding hand sway from light setup.
