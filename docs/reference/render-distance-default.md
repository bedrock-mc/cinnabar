# Default render distance

The ordinary render-distance policy targets Bedrock 1.26.50.26. Saved user
choices override the device recommendation. Resetting Video settings restores
the current device recommendation.

| Input or operation | Rule |
| --- | --- |
| RAM | Installed physical bytes, not free memory. |
| RAM level limit | Truncate `sqrt(((RAM >> 10) - 1,572,864) / 2,560)` after unsigned subtraction and integer division; clamp to 5–96. |
| Shared-memory classification | Dedicated graphics memory below 350 MiB. |
| Shared-memory recommendation limit | Smaller of the RAM level limit and 16. |
| Separate graphics-memory limit | Round `sqrt(graphics bytes / 514,718.5546875)`; ordinary desktop uses half the dedicated graphics bytes. Platform type 2 uses all of them. |
| Recommendation | Take the smallest RAM, graphics and 96-chunk limits; raise to at least 7; choose `5 + floor((limit - 5) / 2)`, bounded by the available RAM levels. |
| Ordinary levels | Every integer from 5 through the RAM level limit; graphics memory affects the recommendation, not the list. |
| Initialization | Compute at option-registry construction; saved preferences replace the guess. |

Cinnabar extends the same one-chunk steps to its intentional 255-chunk maximum.
It preserves an explicitly saved 4 from the old settings range. The device
recommendation is not stored as a user choice; unrelated settings saves leave
it automatic. Missing RAM uses 5. Missing graphics memory uses the shared-memory
recommendation and logs the unavailable probe.

The native adapter probes use DXGI dedicated memory on Windows, discrete Vulkan
memory heaps on Linux, and the selected Metal device's registry VRAM on macOS.
Unified Metal memory is shared. Other rendering backends use the logged fallback.
This table covers ordinary rendering; experimental low-memory overrides and
advanced graphics preset policies are not yet verified.
