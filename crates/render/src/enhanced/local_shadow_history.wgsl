#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import cinnabar::enhanced_common::{EnhancedFrame,FEATURE_SHADOWS,decode_geometric_normal}
#import cinnabar::enhanced_ao::ao_world_position
#import cinnabar::enhanced_local_lights::{LightSources,point_light_visibility_quality}
#import cinnabar::enhanced_actor_motion::stationary_receiver_motion
#import cinnabar::enhanced_shadow::sun_shadow_sample
#import cinnabar::enhanced_sun_shadow_temporal::{sun_shadow_receiver_plane,sun_shadow_history_sample,local_shadow_history_sample,shadow_receiver_depth_tolerance}

@group(0) @binding(0) var<uniform> frame:EnhancedFrame;
@group(0) @binding(1) var depth_texture:texture_depth_2d;
@group(0) @binding(2) var previous_visibility:texture_2d<f32>;
@group(0) @binding(3) var linear_sampler:sampler;
@group(0) @binding(4) var shadow_map:texture_depth_2d_array;
@group(0) @binding(5) var<storage,read> lights:LightSources;
@group(0) @binding(6) var<uniform> policy:vec4<f32>;
@group(0) @binding(7) var motion_texture:texture_2d<f32>;
@group(0) @binding(8) var shadow_sampler:sampler_comparison;
@group(0) @binding(10) var previous_sun_visibility:texture_2d<f32>;
@group(0) @binding(11) var sun_shadow_map:texture_depth_2d_array;
@group(0) @binding(12) var<uniform> sun_policy:vec4<f32>;
@group(0) @binding(13) var receiver_normal_texture:texture_2d<f32>;

fn local_shadow_mix(current:vec2<f32>,history:vec4<f32>,expected:f32,parameters:vec4<f32>)->vec2<f32> {
    if(parameters.x<0.5 || history.z<0.5 || expected<=0.0 || abs(history.w-expected)>shadow_receiver_depth_tolerance(expected)) {return current;}
    return mix(clamp(history.xy,vec2(0.0),vec2(1.0)),current,parameters.y);
}

fn local_shadow_mix4(current:vec4<f32>,history:vec4<f32>,metadata:vec2<f32>,expected:f32,parameters:vec4<f32>)->vec4<f32> {
    if(parameters.x<0.5 || metadata.x<0.5 || expected<=0.0 || abs(metadata.y-expected)>shadow_receiver_depth_tolerance(expected)) {return current;}
    return mix(clamp(history,vec4(0.0),vec4(1.0)),current,parameters.y);
}

struct LocalShadowOutput {
    @location(0) visibility:vec4<f32>,
    @location(1) metadata:vec4<f32>,
    @location(2) sunlight:vec4<f32>,
}

fn local_shadow_output(visibility:vec4<f32>,metadata:vec4<f32>,sunlight:vec4<f32>)->LocalShadowOutput {
    var result:LocalShadowOutput;
    result.visibility=visibility;
    result.metadata=metadata;
    result.sunlight=sunlight;
    return result;
}

@group(0) @binding(9) var previous_metadata:texture_2d<f32>;

@fragment fn resolve_local_shadows(in:FullscreenVertexOutput)->LocalShadowOutput {
    let size=vec2<i32>(textureDimensions(depth_texture));
    let pixel=clamp(vec2<i32>(in.uv*vec2<f32>(size)),vec2(0),size-vec2(1));
    let uv=(vec2<f32>(pixel)+vec2(0.5))/vec2<f32>(size);
    let depth=textureLoad(depth_texture,pixel,0);
    if(depth<=0.00001) {return local_shadow_output(vec4(1.0),vec4(0.0),vec4(1.0,0.0,0.0,0.0));}
    let world=ao_world_position(frame,uv,depth);
    let encoded_normal=textureLoad(receiver_normal_texture,pixel,0).rg;
    let normal=decode_geometric_normal(encoded_normal);
    let filter_taps=u32(max(frame.quality.w,0.0));
    let blocker_taps=u32(max(frame.quality.z,0.0));
    let local_valid=lights.info.z!=0u && lights.info.w!=0u;
    var visibility=vec4(1.0);
    if(local_valid){
        for(var index=0u;index<min(lights.info.w,4u);index+=1u){
            visibility[index]=point_light_visibility_quality(lights.lights[index],lights.info.y,world,normal,shadow_map,shadow_sampler,filter_taps,blocker_taps);
        }
    }
    var sunlight=vec4(1.0,0.0,0.0,0.0);
    if((frame.flags.x&FEATURE_SHADOWS)!=0u){
        let bias_normal=select(-normal,normal,dot(normal,frame.light_direction.xyz)>=0.0);
        let sample=sun_shadow_sample(frame,sun_shadow_map,shadow_sampler,world,bias_normal,vec2<f32>(pixel));
        sunlight=vec4(sample.x,encoded_normal,select(0.0,min(frame.projection.x/depth,60000.0),sample.y>=0.0));
    }
    let previous=frame.previous_clip_from_world*vec4(world,1.0);
    let previous_uv=previous.xy/max(previous.w,0.00001)*vec2(0.5,-0.5)+vec2(0.5);
    let motion=textureLoad(motion_texture,pixel,0);
    let stationary=stationary_receiver_motion(motion,previous_uv-uv,frame.viewport.xy);
    if(stationary && previous.w>0.0 && all(previous_uv>vec2(0.0)) && all(previous_uv<vec2(1.0))){
        if(sun_policy.x>0.5 && sunlight.a>0.0){
            let plane=sun_shadow_receiver_plane(frame.previous_clip_from_world,world,normal);
            let old=sun_shadow_history_sample(previous_sun_visibility,plane,normal,frame.projection.x,frame.viewport.xy);
            sunlight.r=mix(sunlight.r,old.x,(1.0-sun_policy.y)*old.y);
        }
        if(local_valid && policy.x>0.5){
            let plane=sun_shadow_receiver_plane(frame.previous_clip_from_world,world,normal);
            let old=local_shadow_history_sample(previous_visibility,previous_metadata,plane,normal,frame.projection.x,frame.viewport.xy);
            let lanes=u32(policy.z);let retained=u32(policy.w);
            for(var lane=0u;lane<min(lights.info.w,4u);lane+=1u){
                if((retained&(1u<<lane))==0u){continue;}
                let old_lane=(lanes>>(lane*2u))&3u;
                visibility[lane]=mix(visibility[lane],old.visibility[old_lane],(1.0-policy.y)*old.confidence);
            }
        }
    }
    return local_shadow_output(visibility,vec4(select(0.0,1.0,local_valid),min(frame.projection.x/max(depth,0.00001),60000.0),encoded_normal),sunlight);
}
