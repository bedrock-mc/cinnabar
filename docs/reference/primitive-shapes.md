# Server primitive shapes

The packet carries ordered changes for network-owned debug geometry. These rules describe
Bedrock 1.26.50. The implementation uses retained instance slots and shared meshes; its remaining
parity work is tracked in `plan.md`.

## Vanilla rules

| Surface | Rule |
| --- | --- |
| Identity | Entries address an unsigned 64-bit network id. A present shape type creates an unknown id or patches its existing shape. An absent shape type removes that id; removing an unknown id does nothing. Packet order is significant. |
| Patches | Omitted common fields preserve their old values. Existing ids retain their geometry type. Only an extra payload matching the existing geometry type changes its type-specific settings; unrelated extra payloads are unused. |
| Creation defaults | Position and rotation are zero, scale is one, color is opaque white, dimension is all dimensions, actor attachment and lifetime are absent, and no shape-specific distance override is set. |
| Dimensions | Selector `3` means every dimension. Other selectors render only when equal to the player's current dimension. Changing dimensions does not delete the stored shape. |
| Attachment | Actor ids are unique ids. `-1` detaches; an omitted attachment preserves it. Attached geometry adds the actor's interpolated riding position minus its vertical offset (feet position) to its coordinates without applying actor rotation. Missing actors suppress rendering until found. |
| Lifetime | Zero time-left clears the optional lifetime; an omitted time-left preserves it. The server subtracts elapsed monotonic seconds on its ticks and emits a removal when the result is at most zero. The client stores the time-left but its primitive rendering system waits for that removal. |
| Distance | Omission preserves the previous limit. A negative value clears the override. Visibility requires squared distance strictly below the squared limit, so zero hides even a coincident shape. Without an override the adjusted player render distance is used. The distance origin includes actor attachment; boxes use their lower corner, other shapes their location. |
| Wire color | The four-byte signed integer contains ARGB: red in bits 16–23, green in 8–15, blue in 0–7, alpha in 24–31. Channels become floats divided by 255; alpha is preserved. |
| Line | Two endpoints, initially both zero. The first is the common location, the second is the line's absolute end location. Common scale and rotation do not alter the line. |
| Box | Twelve edges, centered at location. Full side lengths are component-wise bounds multiplied by common scale; default bounds are `(1,1,1)`. Common rotation does not rotate the box. |
| Sphere | Three orthogonal great circles with common scale as radius. Each circle uses the requested segment count, initially 20; a sphere has `6 × segments` line vertices for positive segment counts; zero segments produces six zero-valued vertices forming three degenerate lines. Common rotation is unused. |
| Circle | A horizontal ring with common scale as radius and `2 × segments` line vertices for positive segment counts; zero segments produces two zero-valued vertices forming a degenerate line; default segment count is 20. Common rotation is unused. |
| Arrow | Absolute start and end positions, initially zero. The head defaults to length 1, radius 0.5, and 4 segments. Its segment count is clamped to 3–128 by rendering. The head contains the ring and the spokes from both endpoints of each ring edge to the tip; with the shaft it has `6 × segments + 2` line vertices. Common scale and rotation are unused. |
| Ring sampling | Samples the native quantized trigonometry table, closes the final edge, and selects the same perpendicular axes for each great circle. |
| Arrow orientation | Picks the coordinate axis least aligned with the normalized direction, then uses two cross products for the head basis. Those perpendicular vectors are not renormalized. |
| Geometry material | Uses a hardware line list with one-pixel lines, depth comparison LessEqual and depth writes. The debug material has no alpha blending; the packed color's RGB remains visible even when its alpha is zero. |
| Text payload | A text payload replaces text and its facing, background, depth and backface options together. An absent background in that payload removes the custom background override. Text is interpreted as a text object when valid, otherwise as a literal string. |
| Text resolution | Valid text objects are retained and resolved when their render helper is dirty, including after any present-type update even when every field equals the existing value. Input-mode and interaction-model changes also refresh resolution. Ordinary unchanged frames reuse the resolved text. |
| Text lines | Literal backslash-n sequences become newlines. Empty lines are discarded and each remaining line is centered independently using integer half widths. |
| Text defaults | Empty text, billboard facing, no explicit background override, depth testing disabled, background backface enabled, text backface disabled. The nametag helper supplies the ordinary black background at alpha 0.25. |
| Text line gap | The packet contains a line-gap field, but the client text updater does not copy it into its retained shape. The ordinary nametag line pitch remains in effect. |
| Text world size | One font pixel occupies `scale × 1.6 / 60` blocks. The billboard uses the original anchor for facing, then applies a fixed `0.125 × (nonempty line count − 1)` block lift. |
| Text layout | Line pitch is 10 font pixels. Background bounds are `[-maximum_half_width−1, -1, maximum_half_width+1, 10×line_count−1]`; the default background is black with alpha 0.25. Foreground tint does not tint the background. |
| Text material | Backgrounds blend without depth writes; glyphs blend with depth writes. The depth option switches Always to LessEqual. Depth-tested glyphs reject sampled alpha below 0.5 before foreground tint and receive the native constant depth bias. |
| Text backfaces | Background and glyph backface controls are independent. Explicit rotation replaces billboard facing; rotations are degrees, composed X then Y then Z about fixed axes. Both local font axes are negated. |
| Draw ordering | The renderer sorts helpers by an integer priority, which the packet-driven constructors initialize equally. The sort is unstable; coincident overlapping shapes do not have a kind-based ordering rule. |
| Client shape count | The packet receiver has no explicit fixed shape-count cap; it grows its retained shape collection. No 1k/10k/100k visibility limit is imposed by that receiver. |
| Invalid values | The client implementation skips and counts unknown/unsupported kinds, non-finite payload values, and text exceeding the existing UI text resource bound. These are resource and robustness rules, not claimed vanilla visibility limits. Truncated fields and undecodable variant framing remain fatal. |

Geometry, render-state, text layout and timing checks must be considered separately. A matching
packet decoder or deterministic work test does not close a visual parity gate.

Text shares the existing nametag font rasterizer and its finite atlas capacity. Lines that cannot
fit are omitted until a later atlas rebuild; this is an implementation resource bound, not a
vanilla shape-count rule. Text over the protocol UI text bound is skipped and counted.

The installed material asset corroborates the debug depth and blend states but is a nearby
patch version; a version-matched material witness and visual comparison remain open.

The retained implementation follows server-owned lifetime removal. It deliberately does not arm
a local expiry timer from time-left, because doing so would hide delayed server removals early.

Input-mode and interaction-model changes do not yet invalidate parsed debug text by themselves;
the current text resolver exposes no shared authority for those mode changes. This parity gap
does not require per-frame shape scans when that authority becomes available.

## Local fixture

Build `tools/localserver`, then use `tools/cinnabar-mcp` to launch the client with
`headless: true` and connect with `local_server.args: ["-primitive-shapes"]`.
The fixture sends all six kinds, one shape hidden by dimension, and a cyan box attached
to the player. `/shapes update` changes the box color, `/shapes clear` removes the gallery,
and `/shapes` restores it. Keep captures outside git.

The focused `primitive_shape` tests cover the wire decoder, ordered delivery, retained store,
atlas text, shared meshes, shader validation, GPU visibility and actual glyph rasterization.
The `primitive_shapes_frame_cost_bench` test reports 1k/10k/100k instance preparation with
steady input and 10% sparse updates. Run it through the shared build-slot limiter; CI asserts
work counts only. The synthetic baseline rebuilds every transform each frame and assumes a
full instance upload; it excludes geometry generation, actual draw submission and GPU time.
Its churn loop directly edits a vector, while retained churn includes normalized packet
allocation, network-id lookup and dirty-range tracking. It is a cheap baseline, not a measured
vanilla client or an end-to-end frame-budget comparison.

## Measured instance preparation

Apple M3 Pro, macOS 26.5.1, repository optimized development/test profile, 31 samples per
case; medians in microseconds. Sub-0.05 µs steady samples are below useful timer resolution.
These measurements exclude render submission, GPU execution and server decoding.

| Shapes | Baseline steady µs | Retained steady µs | Baseline 10% churn µs | Retained 10% churn µs | Baseline upload bytes | Retained steady bytes | Retained churn bytes |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1,000 | 4.667 | <0.05 | 4.792 | 5.708 | 128,000 | 0 | 12,800 |
| 10,000 | 48.333 | <0.05 | 48.542 | 76.166 | 1,280,000 | 0 | 128,000 |
| 100,000 | 488.417 | <0.05 | 507.250 | 774.917 | 12,800,000 | 0 | 1,280,000 |

Retained churn includes packet allocation and map lookup, so this lower-bound baseline is
still faster on update frames. Retention removes shape-count-dependent preparation from
unchanged frames and cuts the modeled churn upload volume by 90%. The first retained
implementation measured 12.667 / 147.834 / 3,657.375 µs for the same churn cases; updating
entries in place removed redundant map writes and state copies.

A real GPU preparation regression separately checks zero steady allocations, uploads and mesh
rebuilds, and exactly 12,800 bytes in one upload for 100 adjacent changes among 1,000 shapes.
Sparse changes remain bounded to their changed slots; they can require more upload calls.

## Headless integration check

A local fixture run on macOS/Metal at 1920×1080, scale 1, showed all six shapes, centered
multiline text with its translucent background, and the cyan actor-attached box. The other
dimension's box stayed hidden. A color patch changed the ordinary box to magenta and increased
instance rebuilds from eight to nine. Moving the player moved only the attached box while
rebuilds stayed at nine. Clearing the gallery left zero retained shapes and active mesh batches.
The still frames are external artifacts; this is not a version-matched native visual comparison.
