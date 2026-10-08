#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import cinnabar::enhanced_common::EnhancedFrame
#import cinnabar::enhanced_atmosphere::{environment_sky, atmosphere_stars, sky_view_uv, sky_view_ray}
#import cinnabar::enhanced_clouds::{integrate_clouds, CLOUD_MAX_RANGE}
@group(0) @binding(0) var<uniform> frame:EnhancedFrame;
@group(0) @binding(1) var source:texture_2d<f32>;
@group(0) @binding(2) var linear_sampler:sampler;
@group(0) @binding(3) var depth:texture_depth_2d;
@group(0) @binding(4) var sky_texture:texture_2d<f32>;

@fragment fn capture_probe(in:FullscreenVertexOutput)->@location(0) vec4<f32> {
    let size=vec2<i32>(textureDimensions(depth));
    let d=textureLoad(depth,clamp(vec2<i32>(in.uv*vec2<f32>(size)),vec2(0),size-vec2(1)),0);
    var rgb=textureSampleLevel(source,linear_sampler,in.uv,0.0).rgb;
    if (d<=0.00001 && frame.atmosphere.x>0.5) {
        let h=frame.world_from_clip*vec4(in.uv*vec2(2.0,-2.0)+vec2(-1.0,1.0),1.0,1.0);
        let ray=normalize(h.xyz/h.w-frame.camera_time.xyz);
        let cached_sky=textureSampleLevel(sky_texture,linear_sampler,sky_view_uv(ray),0.0);
        var sky=cached_sky.rgb;
        if(cached_sky.a<=0.5){sky=environment_sky(frame,ray);}
        // Celestial specular is evaluated by the directional BRDF, outside the environment integral.
        sky+=atmosphere_stars(frame,ray);
        var cloud_frame=frame;
        cloud_frame.flags.w=min(select(8u,frame.flags.w,frame.flags.w>0u),8u);
        cloud_frame.temporal.y=0.0;
        let clouds=integrate_clouds(cloud_frame,ray,8192.0,in.position.xy);
        rgb=clouds.rgb+sky*(1.0-clouds.a);
    }
    return vec4(min(max(rgb,vec3(0.0)),vec3(60000.0)),1.0);
}

@fragment fn filter_mip(in:FullscreenVertexOutput)->@location(0) vec4<f32> {
    return textureSampleLevel(source,linear_sampler,in.uv,0.0);
}

// Incident sky excludes geometry bounce and retains the physical downward atmosphere directions.
@fragment fn cloud_environment(in:FullscreenVertexOutput)->@location(0) vec4<f32> {
    let ray=sky_view_ray(in.uv);
    let sky=textureSampleLevel(source,linear_sampler,in.uv,0.0).rgb;
    let angular_footprint=max(length(dpdx(ray)),length(dpdy(ray)));
    var cloud_frame=frame;
    cloud_frame.flags.w=clamp(frame.flags.w/4u,4u,8u);
    cloud_frame.temporal.y=0.0;
    cloud_frame.viewport.w=angular_footprint/1.5;
    let clouds=integrate_clouds(cloud_frame,ray,CLOUD_MAX_RANGE,in.position.xy);
    return vec4(min(max(clouds.rgb+sky*(1.0-clouds.a),vec3(0.0)),vec3(60000.0)),1.0);
}

@fragment fn resolve_reflections(in:FullscreenVertexOutput)->@location(0) vec4<f32> {
    return textureSampleLevel(source,linear_sampler,in.uv,0.0);
}
