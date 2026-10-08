# Zeqa movement fixture

`zeqa_vertical_motion.json` contains only numeric movement fields extracted from
`client-2026-10-01.log`, ticks 33331–33338. The input trace labels resolved movement
as `pos_delta`; the regression keeps that captured value as `delta_y`, while the
fixed packet encoder sends end-of-tick velocity.

The regression isolates Y on a synthetic flat floor. Correction velocity was not
logged and is inferred in the test from consecutive authoritative Y positions.
This fixture does not recreate unknown collision geometry or missing packets.
