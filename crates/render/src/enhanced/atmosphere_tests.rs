//! Native GPU regressions exercise the production horizon and cloud functions.

use super::frame::{LUNAR_IRRADIANCE, SOLAR_IRRADIANCE, light_state};
use super::post_regressions::{execute, execute_with_clouds};

fn calibrated_source(source: &str) -> String {
    let night = light_state(&crate::AtmosphereFrame::from_bedrock_time(
        crate::atmosphere::BEDROCK_DAY_TICKS * 0.75,
        0.0,
        0.0,
    ));
    source.replace("var frame:EnhancedFrame;", &format!(
        "var frame:EnhancedFrame;\nframe.projection.y={SOLAR_IRRADIANCE};\nframe.projection.z={LUNAR_IRRADIANCE};\nframe.light_colour.w={};",
        night.ambient,
    ))
}

fn execute_calibrated(source: &str, count: usize) -> Option<Vec<[f32; 4]>> {
    execute(&calibrated_source(source), count)
}

pub(super) fn execute_calibrated_with_clouds(source: &str, count: usize) -> Option<Vec<[f32; 4]>> {
    execute_with_clouds(&calibrated_source(source), count)
}

#[test]
fn enlarged_celestial_discs_keep_irradiance_and_lunar_phases() {
    let source = r#"
#import cinnabar::enhanced_common::EnhancedFrame
#import cinnabar::enhanced_atmosphere::{ATM_PI,ATM_SUN_RADIUS,ATM_MOON_RADIUS,atmosphere_discs,atmosphere_position,atmosphere_light_transport,atmosphere_solar_source,atmosphere_lunar_source}
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,8>;
@compute @workgroup_size(1) fn regression(){
    var frame:EnhancedFrame;
    frame.atmosphere.x=1.0;
    frame.camera_time.y=64.0;
    frame.viewport.w=1.0/2160.0;
    frame.celestial=vec4(0.0,1.0,0.0,0.0);
    let sun_outer=ATM_SUN_RADIUS*1.25;
    let sun_outside=ATM_SUN_RADIUS*1.8;
    results[0]=vec4(atmosphere_discs(frame,vec3(0.0,1.0,0.0)),1.0);
    results[1]=vec4(atmosphere_discs(frame,vec3(sin(sun_outer),cos(sun_outer),0.0)),1.0);
    results[2]=vec4(atmosphere_discs(frame,vec3(sin(sun_outside),cos(sun_outside),0.0)),1.0);
    let transport=atmosphere_light_transport(atmosphere_position(frame.camera_time.xyz),vec3(0.0,1.0,0.0),0.0);
    let sun_area=ATM_PI*pow(ATM_SUN_RADIUS*1.5,2.0);
    results[6].x=results[0].x*sun_area/(atmosphere_solar_source(frame).x*transport.x);
    frame.celestial.y=-1.0;
    let moon_outer=ATM_MOON_RADIUS*1.25;
    let moon_outside=ATM_MOON_RADIUS*1.8;
    results[3]=vec4(atmosphere_discs(frame,vec3(0.0,1.0,0.0)),1.0);
    results[4]=vec4(atmosphere_discs(frame,vec3(sin(moon_outer),cos(moon_outer),0.0)),1.0);
    results[5]=vec4(atmosphere_discs(frame,vec3(sin(moon_outside),cos(moon_outside),0.0)),1.0);
    let moon_area=ATM_PI*pow(ATM_MOON_RADIUS*1.5,2.0);
    results[6].y=results[3].x*moon_area/(atmosphere_lunar_source(frame).x*transport.x);
    frame.celestial.w=4.0;
    results[7]=vec4(atmosphere_discs(frame,vec3(0.0,1.0,0.0)),1.0);
}
"#;
    let Some(values) = execute_calibrated(source, 8) else {
        return;
    };
    for (centre, outer, outside) in [(0, 1, 2), (3, 4, 5)] {
        assert!(values[centre][0] > 0.0);
        assert!(
            values[outer][0] > values[centre][0] * 0.1,
            "the disc must extend beyond the old angular radius: {values:?}"
        );
        assert_eq!(values[outside][..3], [0.0; 3]);
    }
    for normalized in &values[6][..2] {
        assert!(
            (normalized - 1.0).abs() < 0.001,
            "disc size must preserve source energy"
        );
    }
    assert_eq!(values[7][..3], [0.0; 3], "new moon stays dark");
}

#[test]
fn sky_view_coordinates_reconstruct_both_hemispheres() {
    let source = r#"
#import cinnabar::enhanced_atmosphere::{sky_view_uv,sky_view_ray}
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,4>;
@compute @workgroup_size(1) fn regression(){
    let directions=array<vec3<f32>,4>(vec3(0.2,0.7,0.1),vec3(-0.8,-0.2,0.4),
        vec3(0.0,1.0,0.0),vec3(0.0,-1.0,0.0));
    for(var index=0u;index<4u;index+=1u){
        let ray=normalize(directions[index]);
        let uv=sky_view_uv(ray);
        results[index]=vec4(dot(ray,sky_view_ray(uv)),uv,1.0);
    }
}
"#;
    let Some(values) = execute_calibrated(source, 4) else {
        return;
    };
    for value in values {
        assert!(
            value[0] > 0.9999,
            "sky cache direction must round-trip: {value:?}"
        );
        assert!((0.0..=1.0).contains(&value[1]) && (0.0..=1.0).contains(&value[2]));
    }
}

#[test]
fn missing_terrain_reveals_continuous_atmosphere_instead_of_beige_ground() {
    let source = r#"
#import cinnabar::enhanced_common::EnhancedFrame
#import cinnabar::enhanced_atmosphere::{atmospheric_sky_background,atmosphere_background_ray}
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,6>;
@compute @workgroup_size(1) fn regression(){
    var frame:EnhancedFrame;
    frame.camera_time=vec4(0.0,64.0,0.0,0.0);
    frame.celestial=vec4(0.0,1.0,0.0,0.0);
    frame.atmosphere.x=1.0;
    results[0]=vec4(atmospheric_sky_background(frame,normalize(vec3(1.0,-0.12,0.0))),1.0);
    results[1]=vec4(atmospheric_sky_background(frame,normalize(vec3(1.0,-0.05,0.0))),1.0);
    results[2]=vec4(atmospheric_sky_background(frame,normalize(vec3(1.0,-0.0046,0.0))),1.0);
    results[3]=vec4(atmospheric_sky_background(frame,normalize(vec3(1.0,-0.0044,0.0))),1.0);
    results[4]=vec4(atmosphere_background_ray(frame,vec3(0.0,-1.0,0.0)),1.0);
    frame.atmosphere.x=0.0;
    frame.sky_horizon=vec4(0.4,0.3,0.2,1.0);
    frame.sky_zenith=vec4(0.1,0.2,0.3,1.0);
    results[5]=vec4(atmospheric_sky_background(frame,vec3(0.0,-1.0,0.0)),1.0);
}
"#;
    let Some(values) = execute_calibrated(source, 6) else {
        return;
    };
    for (a, b) in [(0, 1), (2, 3)] {
        let change = distance(values[a], values[b]);
        let magnitude = length(values[a]);
        assert!(
            change < magnitude * 0.08,
            "horizon scattering must stay continuous: {values:?}"
        );
    }
    assert!(
        values[0][2] > values[0][0],
        "the noon lower hemisphere must retain atmospheric blue, not ground albedo"
    );
    assert!(values[4].iter().all(|channel| channel.is_finite()));
    assert!((length(values[4]) - 1.0).abs() < 0.001);
    for (actual, expected) in values[5][..3].iter().zip([0.4, 0.3, 0.2]) {
        assert!((actual - expected).abs() < 0.0001);
    }
}

#[test]
fn night_sky_retains_moonlit_detail_with_bounded_radiance() {
    let source = r#"
#import cinnabar::enhanced_common::EnhancedFrame
#import cinnabar::enhanced_atmosphere::atmospheric_sky_background
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,3>;
@compute @workgroup_size(1) fn regression(){
    var frame:EnhancedFrame;
    frame.camera_time=vec4(0.0,64.0,0.0,0.0);
    frame.atmosphere.x=1.0;
    frame.celestial=vec4(0.0,1.0,0.0,0.0);
    results[0]=vec4(atmospheric_sky_background(frame,vec3(0.0,1.0,0.0)),1.0);
    frame.celestial.y=-1.0;
    results[1]=vec4(atmospheric_sky_background(frame,vec3(0.0,1.0,0.0)),1.0);
    frame.celestial.w=4.0;
    results[2]=vec4(atmospheric_sky_background(frame,vec3(0.0,1.0,0.0)),1.0);
}
"#;
    let Some(values) = execute_calibrated(source, 3) else {
        return;
    };
    assert!(values.iter().flatten().all(|channel| channel.is_finite()));
    assert!(luminance(values[1]) > luminance(values[2]) * 1.1);
    assert!(luminance(values[1]) < luminance(values[0]) * 0.1);
    assert!(luminance(values[2]) > 0.0, "airglow prevents a zero sky");
}

#[test]
fn finite_world_haze_matches_the_background_at_the_horizontal_cutoff() {
    let source = r#"
#import cinnabar::enhanced_common::EnhancedFrame
#import cinnabar::enhanced_atmosphere::{finite_world_haze,physical_aerial}
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,4>;
@compute @workgroup_size(1) fn regression(){
    var frame:EnhancedFrame;
    frame.camera_time=vec4(0.0,400.0,0.0,0.0);
    frame.celestial=vec4(0.0,1.0,0.0,0.0);
    frame.atmosphere=vec4(1.0,0.0,230.0,0.0);
    results[0]=vec4(finite_world_haze(frame,vec3(32.0,64.0,0.0)),
        finite_world_haze(frame,vec3(200.0,64.0,0.0)),
        finite_world_haze(frame,vec3(230.0,64.0,0.0)),
        finite_world_haze(frame,vec3(200.0,4096.0,0.0)));
    let horizon=vec3(0.25,0.45,0.7);
    results[1]=vec4(physical_aerial(frame,vec3(8.0,2.0,0.5),vec3(230.0,64.0,0.0),horizon),1.0);
    frame.ambient_colour.w=1.0;
    results[2].x=finite_world_haze(frame,vec3(200.0,64.0,0.0));
    frame.atmosphere.w=1.0;
    results[2].y=finite_world_haze(frame,vec3(230.0,64.0,0.0));
    frame.atmosphere.w=0.0;
    frame.atmosphere.x=0.0;
    results[2].z=finite_world_haze(frame,vec3(230.0,64.0,0.0));
    frame.atmosphere.x=1.0;
    frame.atmosphere.z=460.0;
    results[3].x=finite_world_haze(frame,vec3(230.0,64.0,0.0));
}
"#;
    let Some(values) = execute_calibrated(source, 4) else {
        return;
    };
    assert!(values[0][0].abs() < 0.0001);
    assert!(values[0][1] > 0.0 && values[0][1] < 1.0);
    assert!((values[0][2] - 1.0).abs() < 0.0001);
    assert!((values[0][1] - values[0][3]).abs() < 0.0001);
    assert!((values[2][0] - values[0][1]).abs() < 0.0001);
    assert!(values[2][1].abs() < 0.0001 && values[2][2].abs() < 0.0001);
    assert!(values[3][0].abs() < 0.0001);
    for (actual, expected) in values[1][..3].iter().zip([0.25, 0.45, 0.7]) {
        assert!((actual - expected).abs() < 0.0001);
    }
}

#[test]
fn cloud_projection_retains_resolvable_detail_and_jitter_needs_valid_history() {
    let source = r#"
#import cinnabar::enhanced_common::EnhancedFrame
#import cinnabar::enhanced_clouds::{cloud_density,cloud_sample_jitter,cloud_pixel_footprint,cloud_previous_point,cloud_history_position}
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,38>;
@compute @workgroup_size(1) fn regression(){
    var frame:EnhancedFrame;
    frame.camera_time=vec4(0.0,64.0,0.0,100.0);
    frame.clouds=vec4(0.7,196.0,64.0,72.0);
    frame.viewport.w=1.0/1080.0;
    let pixel=vec2(61.0,37.0);
    let before=cloud_sample_jitter(frame,pixel);
    frame.temporal.x=12.0;
    frame.camera_time.w=200.0;
    let after=cloud_sample_jitter(frame,pixel);
    frame.temporal.y=1.0;
    let temporal=cloud_sample_jitter(frame,pixel);
    frame.camera_time.w=400.0;
    results[0]=vec4(before,after,temporal,cloud_sample_jitter(frame,pixel));
    let footprint=cloud_pixel_footprint(frame,700.0);
    frame.viewport.w*=2.0;
    results[1]=vec4(footprint,cloud_pixel_footprint(frame,700.0),0.0,0.0);
    frame.clouds.w=0.0;
    for(var index=0u;index<32u;index+=1u){
        let point=vec3(80.0+f32(index)*31.0,228.0,37.0+f32(index%5u)*27.0);
        results[2u+index]=vec4(cloud_density(frame,point,footprint),cloud_density(frame,point,128.0),0.0,0.0);
    }
    frame.camera_time.w=100.0;
    frame.clouds.w=72.0;
    frame.temporal.z=0.1;
    results[34]=vec4(cloud_previous_point(frame,vec3(10.0,228.0,20.0)),1.0);
    results[35]=cloud_history_position(frame,vec3(0.0,1.0,0.0),0.25);
    results[36]=cloud_history_position(frame,vec3(0.0,1.0,0.0),1.0);
    frame.camera_time.y=300.0;
    results[37]=cloud_history_position(frame,vec3(0.0,-1.0,0.0),0.25);
}
"#;
    let Some(values) = execute_calibrated_with_clouds(source, 38) else {
        return;
    };
    assert!((values[0][0] - values[0][1]).abs() < 0.0001);
    assert!((values[0][2] - values[0][3]).abs() < 0.0001);
    assert!((values[0][2] - values[0][0]).abs() > 0.001);
    assert!((values[1][1] / values[1][0] - 2.0).abs() < 0.001);
    let detailed = variance(&values[2..34], 0);
    let blurred = variance(&values[2..34], 1);
    assert!(
        detailed > blurred * 1.5 && detailed > 0.00001,
        "resolvable cloud shape must survive the pixel footprint: fine {detailed}, blurred {blurred}"
    );
    assert!(values[34][0] > 10.0 && values[34][2] > 20.0);
    assert!((values[34][1] - 228.0).abs() < 0.0001);
    assert!(values[35][3] > 0.5 && values[37][3] > 0.5);
    assert!(values[35][1] > 196.0 && values[35][1] < 228.0);
    assert!(values[37][1] > 228.0 && values[37][1] < 260.0);
    assert!((values[36][3]).abs() < 0.0001);
}

#[test]
fn cloud_shadow_coordinates_follow_the_same_light_ray_at_receiver_heights() {
    let source = r#"
#import cinnabar::enhanced_common::EnhancedFrame
#import cinnabar::enhanced_atmosphere::{cloud_shadow_uv,cloud_shadow_receiver_height}
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,6>;
@compute @workgroup_size(1) fn regression(){
    var frame:EnhancedFrame;
    frame.camera_time.y=64.0;
    frame.light_direction=vec4(normalize(vec3(1.0,0.2,0.4)),1.0);
    frame.cloud_shadow=vec4(0.0,0.0,512.0,1.0);
    let ground=vec3(30.0,64.0,40.0);
    let elevated=ground+frame.light_direction.xyz*50.0;
    results[0]=vec4(cloud_shadow_uv(frame,ground),cloud_shadow_uv(frame,elevated));
    results[1]=vec4(cloud_shadow_uv(frame,vec3(30.0,96.0,40.0)),0.0,0.0);
    frame.light_direction=vec4(0.0,1.0,0.0,1.0);
    results[2]=vec4(cloud_shadow_uv(frame,ground),cloud_shadow_uv(frame,vec3(30.0,160.0,40.0)));
    frame.light_direction=vec4(1.0,0.0,0.0,0.0);
    results[3]=vec4(cloud_shadow_uv(frame,elevated),0.0,0.0);
    frame.clouds=vec4(0.60,196.0,96.0,0.0);
    frame.light_direction=vec4(normalize(vec3(1.0,0.2,0.4)),1.0);
    frame.camera_time.y=frame.clouds.y;
    results[4]=vec4(cloud_shadow_uv(frame,ground),cloud_shadow_receiver_height(frame),1.0);
    frame.camera_time.y=frame.clouds.y+frame.clouds.z+64.0;
    results[5]=vec4(cloud_shadow_uv(frame,ground),cloud_shadow_receiver_height(frame),1.0);
}
"#;
    let Some(values) = execute_calibrated(source, 6) else {
        return;
    };
    for channel in 0..2 {
        assert!((values[0][channel] - values[0][channel + 2]).abs() < 0.0001);
        assert!((values[2][channel] - values[2][channel + 2]).abs() < 0.0001);
    }
    assert!(values[1][0] < values[0][0] && values[1][1] < values[0][1]);
    assert!(values[3].iter().all(|value| value.is_finite()));
    for channel in 0..3 {
        assert!((values[4][channel] - values[5][channel]).abs() < 0.0001);
    }
    assert!((values[4][2] - 196.0).abs() < 0.0001);
}

#[test]
fn nearby_clear_air_preserves_materials_until_the_streaming_boundary() {
    let source = r#"
#import cinnabar::enhanced_common::EnhancedFrame
#import cinnabar::enhanced_atmosphere::{atmosphere_aerial_transmittance,physical_aerial,finite_world_haze}
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,8>;
@compute @workgroup_size(1) fn regression(){
    var frame:EnhancedFrame;
    frame.camera_time=vec4(0.0,64.0,0.0,0.0);
    frame.celestial=vec4(0.0,1.0,0.0,0.0);
    frame.light_direction=vec4(0.0,1.0,0.0,frame.projection.y);
    frame.light_colour=vec4(vec3(1.0),frame.light_colour.w);
    frame.atmosphere=vec4(1.0,0.0,230.0,0.0);
    let world=vec3(100.0,64.0,0.0);
    let albedo=vec3(0.4,0.2,0.08);
    let horizon=vec3(0.15,0.25,0.45);
    results[0]=vec4(atmosphere_aerial_transmittance(frame,world),finite_world_haze(frame,world));
    results[1]=vec4(physical_aerial(frame,albedo,world,horizon),finite_world_haze(frame,vec3(190.0,64.0,0.0)));
    results[2]=vec4(physical_aerial(frame,albedo,vec3(230.0,64.0,0.0),horizon),1.0);
    frame.ambient_colour.w=1.0;
    results[3]=vec4(atmosphere_aerial_transmittance(frame,world),finite_world_haze(frame,world));
    frame.ambient_colour.w=0.0;
    frame.celestial.y=-1.0;
    frame.light_direction.w=frame.projection.z;
    results[4]=vec4(physical_aerial(frame,albedo,world,horizon),1.0);
    for(var index=0u;index<3u;index+=1u){
        frame.ambient_colour.w=0.49+f32(index)*0.01;
        results[5u+index]=vec4(atmosphere_aerial_transmittance(frame,world),1.0);
    }
}
"#;
    let Some(values) = execute_calibrated(source, 8) else {
        return;
    };
    assert_eq!(values[0][3], 0.0);
    assert_eq!(values[1][3], 0.0, "clear distant buildings retain contrast");
    for channel in 0..3 {
        assert!(values[0][channel] > 0.99 && values[0][channel] <= 1.0);
        assert!(values[3][channel] < values[0][channel]);
        assert!((values[1][channel] - [0.4, 0.2, 0.08][channel]).abs() < 0.008);
        assert!((values[4][channel] - [0.4, 0.2, 0.08][channel]).abs() < 0.008);
        assert!((values[2][channel] - [0.15, 0.25, 0.45][channel]).abs() < 0.0001);
        assert!(
            (values[6][channel] - (values[5][channel] + values[7][channel]) * 0.5).abs() < 0.0001
        );
    }
}

#[test]
fn directional_light_uses_shared_sources_and_atmospheric_extinction_once() {
    let source = r#"
#import cinnabar::enhanced_common::EnhancedFrame
#import cinnabar::enhanced_atmosphere::{atmosphere_solar_source,atmosphere_lunar_source,atmosphere_direct_irradiance,atmosphere_airglow}
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,5>;
@compute @workgroup_size(1) fn regression(){
    var frame:EnhancedFrame;
    frame.camera_time=vec4(0.0,64.0,0.0,0.0);
    frame.celestial=vec4(0.0,1.0,0.0,0.0);
    frame.atmosphere.x=1.0;
    frame.light_colour=vec4(vec3(1.0),frame.light_colour.w);
    frame.light_direction=vec4(0.0,1.0,0.0,frame.projection.y);
    results[0]=vec4(atmosphere_solar_source(frame),1.0);
    results[1]=vec4(atmosphere_direct_irradiance(frame,frame.camera_time.xyz),1.0);
    frame.light_direction.w*=0.5;
    results[2]=vec4(atmosphere_direct_irradiance(frame,frame.camera_time.xyz),1.0);
    frame.celestial.y=-1.0;
    frame.light_direction.w=frame.projection.z;
    results[3]=vec4(atmosphere_direct_irradiance(frame,frame.camera_time.xyz),1.0);
    frame.light_direction.w=0.0;
    results[4]=vec4(dot(atmosphere_direct_irradiance(frame,frame.camera_time.xyz),vec3(1.0)),
        dot(atmosphere_airglow(frame,vec3(0.0,1.0,0.0)),vec3(1.0)),atmosphere_lunar_source(frame).x,1.0);
}
"#;
    let Some(values) = execute_calibrated(source, 5) else {
        return;
    };
    for channel in 0..3 {
        assert!((values[0][channel] - SOLAR_IRRADIANCE).abs() < 0.0001);
        assert!(values[1][channel] > 0.0 && values[1][channel] < values[0][channel]);
        assert!((values[2][channel] / values[1][channel] - 0.5).abs() < 0.0001);
        assert!(
            (values[3][channel] / values[1][channel] - LUNAR_IRRADIANCE / SOLAR_IRRADIANCE).abs()
                < 0.0001
        );
    }
    assert!(values[4][0].abs() < 0.0001);
    assert!(
        values[4][1] > 0.0,
        "zero directional strength retains bounded airglow"
    );
    assert!((values[4][2] - LUNAR_IRRADIANCE).abs() < 0.0001);
}

#[test]
fn exposure_protects_day_texture_highlights_and_adapts_in_logical_time() {
    let mut source = include_str!("exposure.wgsl").to_owned();
    source.push_str(
        r#"
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,4>;
@compute @workgroup_size(1) fn regression(){
    let ordinary=exposure_policy(0.08,1.0,0.0);
    let bright=highlight_exposure_policy(0.08,2.0,1.0,0.0);
    let dark=highlight_exposure_policy(0.008,0.018,0.0,0.0);
    results[0]=vec4(ordinary.x,bright.x,bright.x*2.0,dark.x*0.008);
    results[1]=vec4(adapted_exposure(1.0,0.25,0.0,true),
        adapted_exposure(1.0,0.25,0.02,true),
        adapted_exposure(1.0,0.25,0.04,true),
        adapted_exposure(1.0,0.25,0.02,false));
    let first=adapted_exposure(1.0,0.25,0.02,true);
    results[2].x=adapted_exposure(first,0.25,0.02,true);
    let lamps=highlight_exposure_policy(0.1,4.0,0.0,0.0);
    results[3]=vec4(lamps.x*4.0,exposure_policy(0.1,0.0,0.0).y,
        exposure_policy(0.1,1.0,0.0).y,dark.x);
}
"#,
    );
    let Some(values) = execute(&source, 4) else {
        return;
    };
    assert!(values[0][1] < values[0][0]);
    assert!(
        values[0][2] <= 0.8001,
        "ordinary highlights stay below a hard shoulder"
    );
    assert!(values[0][3] > 0.02 && values[0][3] < 0.055);
    assert!((values[1][0] - 1.0).abs() < 0.0001);
    assert!(values[1][1] > values[1][2] && values[1][2] > 0.25);
    assert!((values[1][3] - 0.25).abs() < 0.0001);
    assert!((values[2][0] - values[1][2]).abs() < 0.0001);
    assert!(
        values[3][0] > 1.0,
        "intentional light sources retain HDR radiance"
    );
    assert!(values[3][1] < values[3][2] * 0.5 && values[3][3] < 4.0);
}

#[test]
fn surface_transport_warms_grazing_sunlight_and_occludes_below_ground() {
    let source = r#"
#import cinnabar::enhanced_common::EnhancedFrame
#import cinnabar::enhanced_atmosphere::atmosphere_surface_transport
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,4>;
@compute @workgroup_size(1) fn regression(){
    var frame:EnhancedFrame;
    frame.atmosphere.x=1.0;
    let world=vec3(0.0,64.0,0.0);
    results[0]=vec4(atmosphere_surface_transport(frame,world,vec3(0.0,1.0,0.0)),1.0);
    results[1]=vec4(atmosphere_surface_transport(frame,world,normalize(vec3(1.0,0.02,0.0))),1.0);
    results[2]=vec4(atmosphere_surface_transport(frame,world,vec3(0.0,-1.0,0.0)),1.0);
    frame.ambient_colour.w=1.0;
    results[3]=vec4(atmosphere_surface_transport(frame,world,vec3(0.0,1.0,0.0)),1.0);
}
"#;
    let Some(values) = execute_calibrated(source, 4) else {
        return;
    };
    for channel in 0..3 {
        assert!(values[0][channel] > 0.0 && values[0][channel] <= 1.0);
        assert!(values[1][channel] < values[0][channel]);
        assert!(values[2][channel].abs() < 0.0001);
        assert!(values[3][channel] < values[0][channel]);
    }
    assert!(
        values[1][0] / values[1][2].max(0.0001) > values[0][0] / values[0][2].max(0.0001),
        "longer optical path produces a warmer sun"
    );
}

fn luminance(value: [f32; 4]) -> f32 {
    value[0] * 0.2126 + value[1] * 0.7152 + value[2] * 0.0722
}

fn length(value: [f32; 4]) -> f32 {
    value[..3]
        .iter()
        .map(|channel| channel * channel)
        .sum::<f32>()
        .sqrt()
}

fn distance(a: [f32; 4], b: [f32; 4]) -> f32 {
    length([a[0] - b[0], a[1] - b[1], a[2] - b[2], 0.0])
}

fn variance(values: &[[f32; 4]], channel: usize) -> f32 {
    let mean = values.iter().map(|value| value[channel]).sum::<f32>() / values.len() as f32;
    values
        .iter()
        .map(|value| (value[channel] - mean).powi(2))
        .sum::<f32>()
        / values.len() as f32
}
