# Nether portals and dimension transfer

Vanilla rules:

| Element | Behavior |
| --- | --- |
| Portal surface | Axis-specific thin cuboid using the animated portal material. |
| Ambient effects | Fixed-tick particles and ambient sound follow physical portal contact. |
| Overlay | Progress advances by 0.0125 per tick and fades by 0.05; opacity is `0.8p⁴ + 0.2`. |
| Camera | Rotated nonuniform scale distorts the world while the animated hand retains its independent FOV. |
| Transfer packets | Preserve the optional loading ID; loading Start precedes action 14, and loading End follows presentation. |
| Respawn | Searching retains a candidate; ready installs it and sends action 7 with the local actor ID before input resumes. |

The user's fast-transfer policy acknowledges the committed dimension flush
immediately. Local movement stays held until decoded collision data, presented
footing and a fresh GPU frame are ready. Landing dependencies receive priority;
other terrain continues loading in the background. Global input ticks continue.
Visual contact is independent of the local cooldown so repeated entry shows the
overlay whenever the server allows travel.

The JSON-UI loading screen carries its texture and animation dependencies and
uses the original product logo with padding. The user accepts the reported
Nether behavior and macOS/Metal rendering. Focused protocol, loading, scheduler,
camera and respawn regressions cover these corrections. Exact native timing and
the deliberate loading/cooldown deviations remain open parity gates.

Official BDS completed four debug transfers with loading Start-to-End times of
1.142, 0.439, 0.829 and 0.369 seconds. These are regression observations, not
release performance acceptance. A separate BDS WorldCorruption shutdown remains
unresolved; saved worlds are preserved.
