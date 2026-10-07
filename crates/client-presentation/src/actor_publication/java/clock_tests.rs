use super::*;

#[test]
fn bow_clock_keeps_native_large_duration_float_rounding() {
    for (ticks, base) in [(2, 0.0), (15, 13.0), (19, 17.0), (24, 22.0)] {
        for (alpha, fraction) in [(0.0, 0.0), (0.37, 0.3671875), (0.5, 0.5), (1.0, 1.0)] {
            assert_eq!(
                java_use(BOW, Some(BOW), ticks, None, alpha),
                Some(JavaUse::Bow {
                    pull: base + fraction
                }),
                "use tick {ticks}, frame {alpha}"
            );
        }
    }
}

#[test]
fn consume_clock_keeps_native_subtract_then_add_rounding() {
    for (ticks, bits) in [
        (1, 0x4202851e),
        (3, 0x41f50a3d),
        (20, 0x415a147b),
        (31, 0x402851ec),
    ] {
        assert_eq!(
            java_use(
                "minecraft:apple",
                Some("minecraft:apple"),
                ticks,
                Some(32),
                0.37
            ),
            Some(JavaUse::Consume {
                remaining: f32::from_bits(bits),
                duration: 32.0
            }),
            "use tick {ticks}"
        );
    }
}
