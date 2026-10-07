#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import cinnabar::enhanced_common::EnhancedFrame
#import cinnabar::enhanced_ao::{ao_world_position,ao_surface_normal}
#import cinnabar::enhanced_local_lights::{LightSources,point_light_visibility}
#import cinnabar::enhanced_actor_motion::stationary_receiver_motion

@group(0) @binding(0) var<uniform> frame:EnhancedFrame;
@group(0) @binding(1) var depth_texture:texture_depth_2d;
@group(0) @binding(2) var previous_visibility:texture_2d<f32>;
@group(0) @binding(3) var linear_sampler:sampler;
@group(0) @binding(4) var shadow_map:texture_depth_2d_array;
@group(0) @binding(5) var<storage,read> lights:LightSources;
@group(0) @binding(6) var<uniform> policy:vec4<f32>;
@group(0) @binding(7) var motion_texture:texture_2d<f32>;
@group(0) @binding(8) var shadow_sampler:sampler_comparison;

fn local_shadow_mix(current:vec2<f32>,history:vec4<f32>,expected:f32,parameters:vec4<f32>)->vec2<f32> {
    if(parameters.x<0.5 || history.z<0.5 || expected<=0.0 || abs(history.w-expected)>max(0.06,expected*0.01)) {return current;}
    return mix(clamp(history.xy,vec2(0.0),vec2(1.0)),current,parameters.y);
}

@fragment fn resolve_local_shadows(in:FullscreenVertexOutput)->@location(0) vec4<f32> {
    let size=vec2<i32>(textureDimensions(depth_texture));
    let pixel=clamp(vec2<i32>(in.uv*vec2<f32>(size)),vec2(0),size-vec2(1));
    let uv=(vec2<f32>(pixel)+vec2(0.5))/vec2<f32>(size);
    let depth=textureLoad(depth_texture,pixel,0);
    if(depth<=0.00001 || lights.info.z==0u) {return vec4(1.0,1.0,0.0,0.0);}
    let world=ao_world_position(frame,uv,depth);
    let normal=ao_surface_normal(frame,depth_texture,uv,depth);
    var visibility=vec2(1.0);
    for(var index=0u;index<min(lights.info.x,2u);index+=1u){
        visibility[index]=point_light_visibility(lights.lights[index],lights.info.y,world,normal,shadow_map,shadow_sampler);
    }
    let previous=frame.previous_clip_from_world*vec4(world,1.0);
    let previous_uv=previous.xy/max(previous.w,0.00001)*vec2(0.5,-0.5)+vec2(0.5);
    let motion=textureLoad(motion_texture,pixel,0);
    let stationary=stationary_receiver_motion(motion,previous_uv-uv,frame.viewport.xy);
    if(stationary && previous.w>0.0 && all(previous_uv>vec2(0.0)) && all(previous_uv<vec2(1.0))){
        let old_size=vec2<f32>(textureDimensions(previous_visibility));
        let base=floor(previous_uv*old_size-vec2(0.5));
        let fraction=fract(previous_uv*old_size-vec2(0.5));
        let expected=frame.projection.x*previous.w/max(previous.z,0.00001);
        var accumulated=vec2(0.0);var total=0.0;
        for(var y=0u;y<2u;y+=1u){for(var x=0u;x<2u;x+=1u){
            let p=clamp(vec2<i32>(base)+vec2(i32(x),i32(y)),vec2(0),vec2<i32>(old_size)-vec2(1));
            let old=textureLoad(previous_visibility,p,0);
            let axis=vec2(select(1.0-fraction.x,fraction.x,x==1u),select(1.0-fraction.y,fraction.y,y==1u));
            let weight=axis.x*axis.y;
            accumulated+=local_shadow_mix(visibility,old,expected,policy)*weight;total+=weight;
        }}
        visibility=accumulated/max(total,0.00001);
    }
    return vec4(visibility,1.0,frame.projection.x/max(depth,0.00001));
}
