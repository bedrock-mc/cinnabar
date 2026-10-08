//! Native fixtures sample the generated production volume, including its real mip chain.

use super::atmosphere_tests::execute_calibrated_with_clouds;

#[test]
fn volume_tiles_in_three_axes_and_filters_unresolvable_structure() {
    let source = r#"
#import cinnabar::enhanced_clouds::cloud_volume_sample
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,67>;
@compute @workgroup_size(1) fn regression(){
    let coordinate=vec3(0.137,0.321,0.731);
    let reference=cloud_volume_sample(coordinate,0.0);
    results[0]=abs(reference-cloud_volume_sample(coordinate+vec3(1.0,0.0,0.0),0.0));
    results[1]=abs(reference-cloud_volume_sample(coordinate+vec3(0.0,1.0,0.0),0.0));
    results[2]=abs(reference-cloud_volume_sample(coordinate+vec3(0.0,0.0,1.0),0.0));
    for(var index=0u;index<64u;index+=1u){
        let point=vec3(f32(index)*0.037,0.31+f32(index%7u)*0.087,0.43+f32(index%11u)*0.079);
        let fine=cloud_volume_sample(point,0.0);
        let filtered=cloud_volume_sample(point,0.5);
        results[3u+index]=vec4(fine.r,filtered.r,fine.g,filtered.g);
    }
}
"#;
    let Some(values) = execute_calibrated_with_clouds(source, 67) else {
        return;
    };
    for value in &values[..3] {
        assert!(value.iter().all(|difference| *difference < 0.0001));
    }
    for value in &values[3..] {
        assert!(
            value
                .iter()
                .all(|sample| sample.is_finite() && (0.0..=1.0).contains(sample))
        );
    }
    for (fine, filtered) in [(0, 1), (2, 3)] {
        let fine_variance = variance(&values[3..], fine);
        let filtered_variance = variance(&values[3..], filtered);
        assert!(
            fine_variance > 0.0001,
            "volume must carry actual spatial structure"
        );
        assert!(
            filtered_variance < fine_variance * 0.1,
            "mips must integrate unresolved noise"
        );
    }
}

#[test]
fn weather_changes_cloud_shapes_and_erosion_preserves_layer_bounds() {
    let source = r#"
#import cinnabar::enhanced_common::EnhancedFrame
#import cinnabar::enhanced_clouds::{cloud_density,cloud_body,cloud_height_profile,cloud_shape_sample,cloud_silhouette,cloud_weather_sample,cloud_weather_coverage,CLOUD_SHAPE_FREQUENCY,CLOUD_WEATHER_FREQUENCY,CLOUD_WEATHER_DRY_SIGNAL,CLOUD_WEATHER_WET_SIGNAL}
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,72>;
@compute @workgroup_size(1) fn regression(){
    var frame:EnhancedFrame;
    frame.clouds=vec4(0.18,196.0,96.0,0.0);
    results[0]=vec4(cloud_density(frame,vec3(0.0,195.0,0.0),0.5),
        cloud_density(frame,vec3(0.0,293.0,0.0),0.5),
        cloud_height_profile(0.6,0.0,0.0),cloud_height_profile(0.6,1.0,0.0));
    results[1]=vec4(cloud_height_profile(0.8,0.3,0.0),cloud_height_profile(0.8,0.3,1.0),
        cloud_silhouette(0.65,0.0,0.70,0.50,0.10),
        cloud_silhouette(0.90,0.0,0.70,0.50,0.10));
    let weather_period=1.0/CLOUD_WEATHER_FREQUENCY;
    let weather_step=weather_period/8.0;
    var regions=vec4(0.0,0.0,1.0,0.0);
    for(var index=0u;index<64u;index+=1u){
        let point=vec3(f32(index%8u)*weather_step,232.0,f32(index/8u)*weather_step);
        let weather=cloud_weather_sample(point,0.5);
        let coverage=cloud_weather_coverage(0.18,0.0,weather.r);
        regions.x+=select(0.0,1.0,coverage==0.0);
        regions.y+=select(0.0,1.0,coverage>0.0);
        regions.z=min(regions.z,weather.b);
        regions.w=max(regions.w,weather.b);
        frame.clouds.x=0.18;
        frame.ambient_colour.w=0.0;
        let clear=cloud_density(frame,point,0.5);
        let body=cloud_body(frame,point,0.5);
        frame.clouds.x=0.70;
        frame.ambient_colour.w=1.0;
        results[2u+index]=vec4(clear,body,cloud_density(frame,point,0.5),cloud_body(frame,point,0.5));
    }
    let point=vec3(137.0,231.0,731.0);
    let shape=cloud_shape_sample(point,0.5);
    let period=1.0/CLOUD_SHAPE_FREQUENCY;
    results[66]=abs(shape-cloud_shape_sample(point+vec3(period,0.0,0.0),0.5));
    results[67]=abs(shape-cloud_shape_sample(point+vec3(0.0,period,0.0),0.5));
    results[68]=abs(shape-cloud_shape_sample(point+vec3(0.0,0.0,period),0.5));
    let clear_coverage=0.18;
    let border=mix(CLOUD_WEATHER_DRY_SIGNAL,CLOUD_WEATHER_WET_SIGNAL,1.0-clear_coverage);
    results[69]=vec4(cloud_weather_coverage(clear_coverage,0.0,border-0.001),
        cloud_weather_coverage(clear_coverage,0.0,border+0.001),
        cloud_weather_coverage(clear_coverage,0.0,CLOUD_WEATHER_WET_SIGNAL),
        cloud_weather_coverage(0.0,0.0,CLOUD_WEATHER_WET_SIGNAL));
    let weather=cloud_weather_sample(point,0.5);
    let middle=mix(CLOUD_WEATHER_DRY_SIGNAL,CLOUD_WEATHER_WET_SIGNAL,0.5);
    results[70]=vec4(cloud_weather_coverage(clear_coverage,0.0,middle),
        cloud_weather_coverage(0.70,1.0,middle),
        length(weather-cloud_weather_sample(point+vec3(weather_period,0.0,0.0),0.5)),
        length(weather-cloud_weather_sample(point+vec3(0.0,0.0,weather_period),0.5)));
    results[71]=regions;
}
"#;
    let Some(values) = execute_calibrated_with_clouds(source, 72) else {
        return;
    };
    assert_eq!(values[0][..3], [0.0; 3]);
    assert!(values[0][3] > 0.5, "cumulus retains a tall body");
    assert!(
        values[1][1] > values[1][0],
        "storm weather grows a taller layer"
    );
    assert_eq!(
        values[1][2], 0.0,
        "weak cells stop before the height envelope"
    );
    assert!(
        values[1][3] > 0.9,
        "strong cells retain billowing vertical extent"
    );
    let mut clear = 0.0;
    let mut storm = 0.0;
    for value in &values[2..66] {
        assert!(
            value
                .iter()
                .all(|sample| sample.is_finite() && (0.0..=1.0).contains(sample))
        );
        assert!(value[0] <= value[1] + 0.0001 && value[2] <= value[3] + 0.0001);
        clear += value[0];
        storm += value[2];
    }
    assert!(
        storm > clear + 1.0,
        "rain produces contiguous cloud masses instead of clear-weather puffs"
    );
    assert!(
        values[71][0] > 0.0 && values[71][1] > 0.0,
        "clear weather has both empty regional columns and connected wet regions"
    );
    assert!(values[71][3] - values[71][2] > 0.2, "regional type varies");
    for value in &values[66..69] {
        assert!(
            value.iter().all(|difference| *difference < 0.0001),
            "shared world-space shape noise has equal periods in every axis"
        );
    }
    assert_eq!(values[69][0], 0.0);
    assert!(values[69][1] > 0.0 && values[69][1] < 0.02);
    assert!(values[69][2] > 0.9);
    assert_eq!(values[69][3], 0.0);
    assert_eq!(values[70][0], 0.0);
    assert!(values[70][1] > values[70][0]);
    assert!(
        values[70][2..]
            .iter()
            .all(|difference| *difference < 0.0001)
    );
}

#[test]
fn volume_march_self_shadows_without_leaking_beyond_its_interval() {
    let source = r#"
#import cinnabar::enhanced_common::{EnhancedFrame,FEATURE_VOLUMETRIC_CLOUDS}
#import cinnabar::enhanced_clouds::{cloud_density,cloud_light_optical_depth,cloud_shadow,integrate_clouds,CLOUD_WEATHER_FREQUENCY}
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,7>;
@compute @workgroup_size(1) fn regression(){
    var frame:EnhancedFrame;
    frame.atmosphere.x=1.0;
    frame.flags.x=FEATURE_VOLUMETRIC_CLOUDS;
    frame.flags.w=24u;
    frame.clouds=vec4(0.90,196.0,96.0,0.0);
    frame.viewport.w=1.0/1080.0;
    frame.celestial=vec4(0.0,1.0,0.0,0.0);
    frame.light_direction=vec4(0.0,1.0,0.0,frame.projection.y);
    frame.light_colour=vec4(vec3(1.0),frame.light_colour.w);
    var occupied=vec3(0.0,232.0,0.0);
    var maximum=0.0;
    let weather_step=1.0/(8.0*CLOUD_WEATHER_FREQUENCY);
    for(var index=0u;index<64u;index+=1u){
        let point=vec3(f32(index%8u)*weather_step,232.0,f32(index/8u)*weather_step);
        let density=cloud_density(frame,point,0.5);
        if(density>maximum){maximum=density;occupied=point;}
    }
    frame.camera_time=vec4(occupied.x,166.0,occupied.z,0.0);
    results[0]=vec4(maximum,cloud_light_optical_depth(frame,occupied,vec3(0.0,1.0,0.0)),
        cloud_light_optical_depth(frame,occupied+vec3(0.0,96.0,0.0),vec3(0.0,1.0,0.0)),
        cloud_shadow(frame,frame.camera_time.xyz));
    results[1]=integrate_clouds(frame,vec3(0.0,1.0,0.0),1000.0,vec2(61.0,37.0));
    results[2]=integrate_clouds(frame,vec3(0.0,1.0,0.0),29.0,vec2(61.0,37.0));
    results[3]=integrate_clouds(frame,vec3(0.0,-1.0,0.0),1000.0,vec2(61.0,37.0));
    frame.celestial.y=-1.0;
    frame.light_direction.w=frame.projection.z;
    results[4]=integrate_clouds(frame,vec3(0.0,1.0,0.0),1000.0,vec2(61.0,37.0));
    frame.camera_time.y=310.0;
    results[5]=integrate_clouds(frame,vec3(0.0,-1.0,0.0),1000.0,vec2(61.0,37.0));
    frame.flags.x=0u;
    results[6]=integrate_clouds(frame,vec3(0.0,-1.0,0.0),1000.0,vec2(61.0,37.0));
}
"#;
    let Some(values) = execute_calibrated_with_clouds(source, 7) else {
        return;
    };
    assert!(values[0][0] > 0.05 && values[0][1] > 0.0);
    assert_eq!(values[0][2], 0.0);
    assert!(values[0][3] < 0.95 && values[0][3] >= 0.0);
    for index in [1, 4, 5] {
        assert!(
            values[index]
                .iter()
                .all(|sample| sample.is_finite() && *sample >= 0.0)
        );
        assert!(values[index][3] > 0.1 && values[index][3] <= 1.0);
        assert!(luminance(values[index]) > 0.0);
    }
    assert!(
        luminance(values[4]) < luminance(values[1]) * 0.1,
        "night clouds must retain night source units"
    );
    for index in [2, 3, 6] {
        assert_eq!(values[index], [0.0; 4]);
    }
}

#[test]
fn cloud_cores_keep_resolved_detail_and_shadows_integrate_the_same_extinction() {
    let source = r#"
#import cinnabar::enhanced_common::{EnhancedFrame,FEATURE_VOLUMETRIC_CLOUDS}
#import cinnabar::enhanced_clouds::{cloud_structure,cloud_shadow,cloud_path_optical_depth,cloud_light_optical_depth,cloud_scattering,cloud_depth_fraction,cloud_history_position,CLOUD_MAX_RANGE,CLOUD_SHADOW_STEPS,CLOUD_LIGHT_STEPS,CLOUD_WEATHER_FREQUENCY}
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,69>;
@compute @workgroup_size(1) fn regression(){
    var frame:EnhancedFrame;
    frame.atmosphere.x=1.0;
    frame.flags.x=FEATURE_VOLUMETRIC_CLOUDS;
    frame.clouds=vec4(0.60,196.0,96.0,0.0);
    frame.camera_time=vec4(0.0,64.0,0.0,100.0);
    frame.light_direction=vec4(0.0,1.0,0.0,frame.projection.y);
    var occupied=vec3(0.0,215.2,0.0);
    var maximum=0.0;
    let weather_step=1.0/(8.0*CLOUD_WEATHER_FREQUENCY);
    for(var index=0u;index<64u;index+=1u){
        let point=vec3(f32(index%8u)*weather_step,215.2,f32(index/8u)*weather_step);
        let structure=cloud_structure(frame,point,0.5);
        let filtered=cloud_structure(frame,point,128.0);
        results[index]=vec4(structure,filtered);
        if(structure.y>maximum){maximum=structure.y;occupied=point;}
    }
    let receiver=vec3(occupied.x,64.0,occupied.z);
    let expected=exp(-cloud_path_optical_depth(frame,receiver,frame.light_direction.xyz,CLOUD_MAX_RANGE,CLOUD_SHADOW_STEPS));
    results[64]=vec4(cloud_shadow(frame,receiver),expected,maximum,0.0);
    frame.light_direction.w=0.0;
    results[64].w=cloud_shadow(frame,receiver);
    results[65]=vec4(cloud_scattering(vec3(1.0),0.0,1.0),cloud_scattering(vec3(1.0),2.0,1.0),
        cloud_scattering(vec3(1.0),8.0,1.0),0.0);
    results[66]=vec4(cloud_depth_fraction(1.0),cloud_depth_fraction(0.85),
        cloud_depth_fraction(0.25),cloud_depth_fraction(0.01));
    results[67]=vec4(cloud_history_position(frame,vec3(0.0,1.0,0.0),0.85).w,
        cloud_history_position(frame,vec3(0.0,1.0,0.0),0.995).w,
        cloud_history_position(frame,vec3(0.0,1.0,0.0),0.85).y,
        cloud_history_position(frame,vec3(0.0,1.0,0.0),0.25).y);
    let grazing=normalize(vec3(0.7,0.03,0.2));
    frame.clouds.x=0.90;
    frame.clouds.z=256.0;
    let low_point=vec3(occupied.x,frame.clouds.y+frame.clouds.z*0.2,occupied.z);
    results[68]=vec4(cloud_light_optical_depth(frame,low_point,grazing),
        cloud_path_optical_depth(frame,low_point,grazing,CLOUD_MAX_RANGE,CLOUD_LIGHT_STEPS),
        cloud_path_optical_depth(frame,low_point,grazing,1200.0,CLOUD_LIGHT_STEPS),1.0);
}
"#;
    let Some(values) = execute_calibrated_with_clouds(source, 69) else {
        return;
    };
    let resolved = &values[..64];
    assert!(
        values[64][2] > 0.65,
        "resolved clouds need opaque cores, not only thin fog"
    );
    assert!(
        resolved
            .iter()
            .any(|sample| sample[0] > 0.05 && sample[0] < 0.60 && sample[1] < sample[0] - 0.01)
    );
    assert!(variance(resolved, 1) > variance(resolved, 3) * 1.5);
    assert!((values[64][0] - values[64][1]).abs() < 0.0001);
    assert_eq!(
        values[64][3], 1.0,
        "no direct source requires no cloud shadow work"
    );
    assert!(values[65][0] > values[65][1] && values[65][1] > values[65][2]);
    assert!(
        values[65][2] < values[65][0] * 0.05,
        "cloud interiors retain depth contrast"
    );
    assert!(values[66].windows(2).all(|pair| pair[0] > pair[1]));
    assert!((values[66][0] - 0.5).abs() < 0.0001);
    assert_eq!(
        values[67][0], 1.0,
        "thin clouds follow wind rather than clear-sky motion"
    );
    assert!(values[67][1] > 0.0 && values[67][1] < 1.0);
    assert!(
        values[67][2] > values[67][3],
        "opaque clouds reproject their visible front"
    );
    assert!((values[68][0] - values[68][1]).abs() < 0.0001);
    assert!(
        values[68][0] > values[68][2],
        "grazing sunlight integrates outgoing cloud extinction beyond the old cutoff"
    );
}

#[test]
fn cloud_radiance_uses_continuous_solar_and_lunar_irradiance_through_twilight() {
    let source = r#"
#import cinnabar::enhanced_common::{EnhancedFrame,FEATURE_VOLUMETRIC_CLOUDS}
#import cinnabar::enhanced_clouds::{cloud_density,integrate_clouds,CLOUD_WEATHER_FREQUENCY}
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,5>;
@compute @workgroup_size(1) fn regression(){
    var frame:EnhancedFrame;
    frame.atmosphere.x=1.0;
    frame.flags.x=FEATURE_VOLUMETRIC_CLOUDS;
    frame.flags.w=24u;
    frame.clouds=vec4(0.90,196.0,96.0,0.0);
    frame.viewport.w=1.0/1080.0;
    frame.celestial=vec4(0.0,1.0,0.0,0.0);
    frame.light_direction=vec4(0.0,1.0,0.0,frame.projection.y);
    frame.light_colour=vec4(vec3(1.0),frame.light_colour.w);
    var occupied=vec3(0.0,232.0,0.0);var maximum=0.0;
    let weather_step=1.0/(8.0*CLOUD_WEATHER_FREQUENCY);
    for(var index=0u;index<64u;index+=1u){
        let point=vec3(f32(index%8u)*weather_step,232.0,f32(index/8u)*weather_step);
        let density=cloud_density(frame,point,0.5);
        if(density>maximum){maximum=density;occupied=point;}
    }
    frame.camera_time=vec4(occupied.x,166.0,occupied.z,0.0);
    let sources=frame.projection.yz;
    results[0]=integrate_clouds(frame,vec3(0.0,1.0,0.0),1000.0,vec2(61.0,37.0));
    frame.light_direction.w=0.0;
    results[1]=integrate_clouds(frame,vec3(0.0,1.0,0.0),1000.0,vec2(61.0,37.0));
    frame.projection.y=sources.x*0.5;
    frame.projection.z=sources.y*0.5;
    results[4]=integrate_clouds(frame,vec3(0.0,1.0,0.0),1000.0,vec2(61.0,37.0));
    frame.projection.y=sources.x;
    frame.projection.z=sources.y;
    frame.light_direction.w=frame.projection.y;
    frame.celestial=vec4(sqrt(1.0-0.024*0.024),-0.024,0.0,0.0);
    results[2]=integrate_clouds(frame,vec3(0.0,1.0,0.0),1000.0,vec2(61.0,37.0));
    frame.celestial=vec4(sqrt(1.0-0.026*0.026),-0.026,0.0,0.0);
    results[3]=integrate_clouds(frame,vec3(0.0,1.0,0.0),1000.0,vec2(61.0,37.0));
}
"#;
    let Some(values) = execute_calibrated_with_clouds(source, 5) else {
        return;
    };
    assert!(luminance(values[0]) > 0.0);
    for channel in 0..3 {
        assert!(
            (values[1][channel] - values[0][channel]).abs()
                < values[0][channel].max(0.0001) * 0.001,
            "the selected terrain shadow source cannot extinguish independent celestial irradiance"
        );
        let interpolated = values[0][channel] * 0.5;
        assert!(
            (values[4][channel] - interpolated).abs() < interpolated.max(0.0001) * 0.001,
            "cloud radiance scales with the shared celestial irradiance sources"
        );
    }
    assert!(
        (luminance(values[2]) - luminance(values[3])).abs() < luminance(values[2]) * 0.02,
        "clouds must not switch light direction at their own twilight threshold"
    );
    for value in &values {
        assert!(
            value
                .iter()
                .all(|sample| sample.is_finite() && *sample >= 0.0)
        );
    }
}

fn luminance(value: [f32; 4]) -> f32 {
    value[0] * 0.2126 + value[1] * 0.7152 + value[2] * 0.0722
}

fn variance(values: &[[f32; 4]], channel: usize) -> f32 {
    let mean = values.iter().map(|value| value[channel]).sum::<f32>() / values.len() as f32;
    values
        .iter()
        .map(|value| (value[channel] - mean).powi(2))
        .sum::<f32>()
        / values.len() as f32
}
