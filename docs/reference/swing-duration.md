# Swing duration

## Vanilla rules

| Rule | Bedrock | Java 1.7.10 |
| --- | --- | --- |
| Base | Six simulation ticks (0.3 seconds) | Six simulation ticks (0.3 seconds) |
| Haste | Subtract amplifier + 1 | Subtract amplifier + 1 |
| Conduit Power | Use the greater Haste/Conduit level | Does not modify swings |
| Mining Fatigue | Add twice (amplifier + 1) when no speed effect is active | Add twice (amplifier + 1) when Haste is absent |
| Effect changes | Recompute duration each tick without resetting the counter | Recompute duration each tick without resetting the counter |
| Start | Counter starts at -1, then advances once per simulation tick | Counter starts at -1, then advances once per simulation tick |
| Repeat | Read the pre-increment counter and accept at half the current duration | Read the pre-increment counter and accept at half the current duration |
| Admission | Admit this tick’s actions before advancing and publishing its counter | Admit this tick’s actions before advancing and publishing its counter |
| Publication | Counter divided by current duration; return to zero at completion | Counter divided by current duration; return to zero at completion |
| Frames | Interpolate tick progress forward across the final wrap to rest, using the local simulation fraction | Interpolate tick progress forward across the final wrap to rest, using the local simulation fraction |

The client's local effect timeline expires finite effects on committed simulation ticks and admits only effects addressed to the local player in the current dimension. Malformed amplifiers use saturating arithmetic, with a minimum duration of one tick.

Held attempts repeat every four, three and five ticks for durations six, four and eight. A tick attempts the swing before effects expire, then publishes progress after expiry. Catch-up ticks retain both effect phases, and publication retains the final two simulation samples independently of the actor presentation clock. Bedrock packet admission and the selected animation mode use independent counters.

A fresh action waits when the retained unsent tick has already been published. Only an attempt whose own transport batch was refused may retry that published tick; another action's refused swing does not grant permission.

A pressed block swing uses the first eligible tick committed in the current frame. Its own refused batch can retry that exact retained tick when it is still the latest published tick. Older attempts wait for a fresh eligible tick within the pending-input deadline. Held mining then continues through the remaining committed ticks in order; the same first-tick attempt is admitted only once.

Authored attack weights, pre-animation variables, arm channels and held-item channels read the same sampled local swing progress. Frame sampling leaves completed animation state and clip clocks unchanged. Native third-person bodies, held parents and animated persona layers share the physics swing sample, including native posture fallbacks selected under Java mode. Successful custom emotes consume their own sampled pose instead.

Native player and held-item animation sample the interpolated attack progress. First-person attack weights remain active while the final wrap has nonzero progress; frame sampling preserves committed controller state and clip clocks.

The actor presentation clock advances before interaction picking. Final local swing publication follows interaction admission and refreshes local poses without advancing remote actors again. The refresh replaces authored script, controller and clock state from the original tick inputs, and preserves the previous tick’s bone pose. Inventory, screen, view and item-use observations remain captured before outbound actions.

Java torso turning reads each completed local tick's swing progress after admission and effect expiry. Catch-up frames retain those individual samples and their resolved movement and yaw. Torso headings use the local physics fraction; equip, limb and cape-position animation retain their own clock. A changed same-tick retry replaces that torso tick once. Initial idle catch-up frames retain every completed tick before any attack.

Early hand readiness evaluates an uncommitted attachable preview. Final source reuse commits it once; a changed source restores the original geometry and animation state before its final evaluation. Authored variables, controller transitions and clip clocks therefore advance once per published hand frame.
