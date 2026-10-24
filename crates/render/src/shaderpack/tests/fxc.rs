//! Programs through FXC, the DX12 compiler shipping installs use.

use super::*;

/// A DX12 device compiling with FXC, as the client configures it.
fn fxc_gpu() -> Option<GpuContext> {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::DX12;
    descriptor.backend_options.dx12.shader_compiler = wgpu::Dx12Compiler::Fxc;
    device_on(&wgpu::Instance::new(descriptor))
}

/// Inverts `colortex0`, sampling it with implicit derivatives in a loop with a uniform bound
/// and an exit that depends on the samples, as raymarched clouds and fog do.
const GRADIENT_LOOP_INVERT: &str = "#version 120
uniform sampler2D colortex0;
uniform float viewWidth;
varying vec2 texcoord;
void main() {
    vec4 sum = vec4(0.0);
    for (int i = 0; i < int(viewWidth) + 4; i++) {
        sum += texture2D(colortex0, texcoord + vec2(float(i) * 0.001, 0.0));
        if (sum.a >= 4.0) break;
    }
    gl_FragData[0] = vec4(1.0 - sum.rgb / sum.a, 1.0);
}
";

#[test]
fn gradient_samples_in_loops_with_varying_exits_compile_and_run() {
    let Some(gpu) = fxc_gpu() else {
        eprintln!("missing fixture: DX12 adapter for the FXC shader pack frame");
        return;
    };
    let bytes = render(
        &gpu,
        &[
            ("composite.vsh", PASS_VERTEX),
            ("composite.fsh", GRADIENT_LOOP_INVERT),
            ("final.vsh", PASS_VERTEX),
            ("final.fsh", FINAL),
        ],
    );
    assert_pixels(&bytes, |row| {
        if row < 2 {
            [0, 255, 255]
        } else {
            [127, 127, 127]
        }
    });
}

/// A dynamically indexed local array beyond FXC's 4096 temporary registers.
const OVER_FXC_TEMPORARIES: &str = "#version 120
varying vec2 texcoord;
void main() {
    float values[8192];
    for (int i = 0; i < 8192; i++) {
        values[i] = float(i) * texcoord.x;
    }
    gl_FragData[0] = vec4(values[int(texcoord.y * 8191.0)]);
}
";

/// The backend compiler's rejection is an internal device error; it fails the load with the
/// compiler's report instead of reaching the device's uncaptured-error handler.
#[test]
fn a_program_the_backend_compiler_rejects_fails_the_load() {
    let Some(gpu) = fxc_gpu() else {
        eprintln!("missing fixture: DX12 adapter for the FXC shader pack load");
        return;
    };
    let pack = pack(&[
        ("composite.vsh", PASS_VERTEX),
        ("composite.fsh", OVER_FXC_TEMPORARIES),
    ]);
    let error = build(
        &pack,
        "fixture",
        JAVA_DIMENSIONS[0],
        &Default::default(),
        None,
        &gpu,
        &Default::default(),
        &Default::default(),
    )
    .err()
    .expect("FXC rejects the composite program");
    assert!(
        error.starts_with("the device rejected the pack") && error.contains("X4505"),
        "reports FXC's error: {error}"
    );
}
