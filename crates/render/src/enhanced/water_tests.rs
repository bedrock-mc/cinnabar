//! Runs the shared surface and underwater transport functions on a native adapter.

use super::post_regressions::execute;
#[path = "water_texture_tests.rs"]
mod textures;

#[test]
fn water_scattering_requires_incident_light_and_preserves_zero_path() {
    let source = r#"
#import cinnabar::enhanced_water::{water_transport,water_transmittance}
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,5>;
@compute @workgroup_size(1) fn regression(){
    results[0]=vec4(water_transport(vec3(0.0),64.0,vec3(0.0)),1.0);
    results[1]=vec4(water_transport(vec3(0.8,0.5,0.2),0.0,vec3(10.0)),1.0);
    results[2]=vec4(water_transmittance(8.0),1.0);
    results[3]=vec4(water_transport(vec3(0.0),8.0,vec3(1.0)),1.0);
    results[4]=vec4(water_transport(vec3(0.0),8.0,vec3(0.01)),1.0);
}
"#;
    let Some(values) = execute(source, 5) else {
        return;
    };
    assert_eq!(&values[0][..3], &[0.0; 3], "unlit water must not glow cyan");
    for (actual, expected) in values[1][..3].iter().zip([0.8, 0.5, 0.2]) {
        assert!((actual - expected).abs() < 0.00001);
    }
    assert!(values[2][0] < values[2][1] && values[2][1] < values[2][2]);
    for (bright, dim) in values[3][..3].iter().zip(&values[4][..3]) {
        assert!(bright.is_finite() && *bright > 0.0);
        assert!((dim * 100.0 - bright).abs() < 0.00001);
    }
}

#[test]
fn refraction_filter_rejects_sky_and_foreground_at_silhouettes() {
    let source = r#"
#import cinnabar::enhanced_water::water_depth_weight
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,1>;
@compute @workgroup_size(1) fn regression(){
    results[0]=vec4(water_depth_weight(0.05,0.005,0.0),
        water_depth_weight(0.05,0.005,0.05),
        water_depth_weight(0.05,0.005,0.005),
        water_depth_weight(0.05,0.005,0.00499));
}
"#;
    let Some(values) = execute(source, 1) else {
        return;
    };
    assert_eq!(
        values[0][0], 0.0,
        "a sky texel must not make a cyan outline"
    );
    assert_eq!(
        values[0][1], 0.0,
        "near foreground must not leak into distant water"
    );
    assert_eq!(values[0][2], 1.0);
    assert!(values[0][3] > 0.99, "continuous surfaces retain filtering");
}

#[test]
fn water_reflection_budget_preserves_grazing_reflections_and_total_internal_reflection() {
    let source = r#"
#import cinnabar::enhanced_water::{water_fresnel,water_screen_reflection_weight}
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,2>;
@compute @workgroup_size(1) fn regression(){
    let normal=water_fresnel(1.0,1.0/1.333);
    results[0]=vec4(normal,water_fresnel(0.05,1.0/1.333),
        water_fresnel(0.5,1.333),water_screen_reflection_weight(normal,5.0));
    results[1]=vec4(water_screen_reflection_weight(0.8,5.0),
        water_screen_reflection_weight(0.8,96.0),
        water_screen_reflection_weight(0.8,60.0),1.0);
}

"#;
    let Some(values) = execute(source, 2) else {
        return;
    };
    assert!((values[0][0] - 0.02037).abs() < 0.0001);
    assert!(values[0][1] > 0.7);
    assert_eq!(values[0][2], 1.0);
    assert_eq!(
        values[0][3], 0.0,
        "normal incidence uses the cached environment"
    );
    assert_eq!(values[1][0], 1.0);
    assert_eq!(values[1][1], 0.0);
    assert!(values[1][2] > 0.0 && values[1][2] < 1.0);
}

#[test]
fn ssr_hit_confidence_fades_depth_edges_and_rejects_far_thickness() {
    let source = r#"
#import cinnabar::enhanced_water::water_ssr_hit_confidence
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,2>;
@compute @workgroup_size(1) fn regression(){
    let stable=water_ssr_hit_confidence(0.25,8.0,0.2,1.0,0.01,0.2);
    let edge=water_ssr_hit_confidence(0.25,8.0,0.2,1.0,0.1,0.2);
    let disoccluded=water_ssr_hit_confidence(0.25,8.0,0.2,0.02,0.01,0.2);
    let outside=water_ssr_hit_confidence(0.25,8.0,0.2,1.0,0.3,0.2);
    results[0]=vec4(stable,edge,disoccluded,outside);
    results[1]=vec4(
        water_ssr_hit_confidence(0.25,8.0,0.2,1.0,0.02,0.2),
        water_ssr_hit_confidence(0.25,8.0,0.2,1.0,0.08,0.2),
        water_ssr_hit_confidence(0.25,8.0,0.2,1.0,0.14,0.2),
        1.0);
}
"#;
    let Some(values) = execute(source, 2) else {
        return;
    };
    assert!(values[0][0] > values[0][1]);
    assert!(
        values[0][1] > 0.0,
        "depth thickness should fade continuously"
    );
    assert!(
        values[0][2] < values[0][0],
        "disocclusion coverage must reduce confidence"
    );
    assert_eq!(
        values[0][3], 0.0,
        "samples beyond the hit thickness are rejected"
    );
    assert!(values[1][0] > values[1][1] && values[1][1] > values[1][2]);
}

#[test]
fn ripple_slopes_are_small_and_distant_footprints_filter_them() {
    let source = r#"
#import cinnabar::enhanced_water::ripple_normal
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,3>;
@compute @workgroup_size(1) fn regression(){
    results[0]=vec4(ripple_normal(vec2(1.3,7.9),3.8,0.0,true),1.0);
    results[1]=vec4(ripple_normal(vec2(1.3,7.9),3.8,4.0,true),1.0);
    var maximum_slope=0.0;
    for(var index=0u;index<128u;index+=1u){
        let n=ripple_normal(vec2(f32(index)*0.17,f32(index)*0.33),f32(index)*0.27,0.0,true);
        maximum_slope=max(maximum_slope,length(n.xz)/n.y);
    }
    results[2]=vec4(maximum_slope,0.0,0.0,1.0);
}
"#;
    let Some(values) = execute(source, 3) else {
        return;
    };
    assert!(
        values[2][0] < 0.0613,
        "small wave gradients must not create giant specular ovals"
    );
    assert!(values[0][1] > 0.998);
    assert!(values[1][0].abs() < 0.00001 && values[1][2].abs() < 0.00001);
}

#[test]
fn displaced_water_normal_uses_the_derivative_of_its_actual_height() {
    let source = r#"
#import cinnabar::enhanced_water::{water_surface_offset,water_surface_gradient}
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,1>;
@compute @workgroup_size(1) fn regression(){
    let world=vec3(1.3,63.0,7.9);
    let epsilon=0.001;
    let dx=(water_surface_offset(world+vec3(epsilon,0.0,0.0),3.8)
        -water_surface_offset(world-vec3(epsilon,0.0,0.0),3.8))/(2.0*epsilon);
    let dz=(water_surface_offset(world+vec3(0.0,0.0,epsilon),3.8)
        -water_surface_offset(world-vec3(0.0,0.0,epsilon),3.8))/(2.0*epsilon);
    results[0]=vec4(water_surface_gradient(world.xz,3.8),dx,dz);
}
"#;
    let Some(values) = execute(source, 1) else {
        return;
    };
    assert!((values[0][0] - values[0][2]).abs() < 0.00001);
    assert!((values[0][1] - values[0][3]).abs() < 0.00001);
}
