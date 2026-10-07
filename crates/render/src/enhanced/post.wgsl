#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import cinnabar::enhanced_common::{EnhancedFrame, FEATURE_SHAFTS, FEATURE_SSAO, FEATURE_VOLUMETRIC_CLOUDS, FEATURE_WATER, interleaved_gradient_noise}
#import cinnabar::enhanced_atmosphere::{atmospheric_sky_background, atmosphere_discs, atmosphere_stars, physical_aerial, finite_world_haze, sky_view_uv, sky_view_ray,cloud_shadow_receiver_height}
#import cinnabar::enhanced_clouds::{integrate_clouds, cloud_shadow, cloud_history_position}
#import cinnabar::enhanced_ao::{horizon_ao, screen_contact_shadow, ao_world_position, ao_surface_normal}
#import cinnabar::enhanced_shadow::sun_shadow_sample
#import cinnabar::enhanced_water::{water_transport, water_ambient_irradiance}
#import cinnabar::enhanced_temporal::{temporal_ycocg, temporal_rgb, temporal_depth_matches, temporal_transparency_reactivity, temporal_history_sample, temporal_clip, temporal_sharpen, temporal_display_curve}

@group(0) @binding(0) var<uniform> frame:EnhancedFrame;
@group(0) @binding(1) var source_texture:texture_2d<f32>;
@group(0) @binding(2) var linear_sampler:sampler;
@group(0) @binding(3) var opaque_texture:texture_2d<f32>;
@group(0) @binding(4) var shaft_texture:texture_2d<f32>;
@group(0) @binding(5) var depth_texture:texture_depth_2d;
@group(0) @binding(6) var shadow_map:texture_depth_2d_array;
@group(0) @binding(7) var shadow_sampler:sampler_comparison;
@group(0) @binding(8) var history_texture:texture_2d<f32>;
@group(0) @binding(9) var effects_texture:texture_2d<f32>;
@group(0) @binding(10) var sky_texture:texture_2d<f32>;
@group(0) @binding(11) var<storage, read> exposure:vec4<f32>;
@group(0) @binding(12) var motion_texture:texture_2d<f32>;

fn luminance(rgb:vec3<f32>)->f32 {return dot(rgb,vec3(0.2126,0.7152,0.0722));}
fn depth_at(uv:vec2<f32>)->f32 {
    let size=vec2<i32>(textureDimensions(depth_texture));
    return textureLoad(depth_texture,clamp(vec2<i32>(uv*vec2<f32>(size)),vec2(0),size-vec2(1)),0);
}
fn ray_at(uv:vec2<f32>)->vec3<f32> {
    let h=frame.world_from_clip*vec4(uv*vec2(2.0,-2.0)+vec2(-1.0,1.0),1.0,1.0);
    return normalize(h.xyz/h.w-frame.camera_time.xyz);
}
fn sky_uv(ray:vec3<f32>)->vec2<f32> {
    return sky_view_uv(ray);
}
@fragment fn sky_lut(in:FullscreenVertexOutput)->@location(0) vec4<f32> {
    let ray=sky_view_ray(in.uv);
    return vec4(atmospheric_sky_background(frame,ray),1.0);
}
@fragment fn cloud_shadows(in:FullscreenVertexOutput)->@location(0) vec4<f32> {
    let xz=(in.uv-vec2(0.5))*frame.cloud_shadow.z+frame.cloud_shadow.xy;
    let world=vec3(xz.x,cloud_shadow_receiver_height(frame),xz.y);
    return vec4(vec3(cloud_shadow(frame,world)),1.0);
}

@fragment fn light_shafts(in:FullscreenVertexOutput)->@location(0) vec4<f32> {
    if ((frame.flags.x&FEATURE_SHAFTS)==0u || frame.flags.y==0u || frame.atmosphere.w>0.5) {return vec4(0.0);}
    let depth=max(depth_at(in.uv),0.00001);
    let world=ao_world_position(frame,in.uv,depth);
    let ray=world-frame.camera_time.xyz;
    let reach=min(length(ray),frame.cascade_texel.w);
    let direction=normalize(ray);
    let cascade=frame.flags.y-1u;
    let bias=frame.cascade_texel[cascade]*frame.cascade_depth_scale[cascade];
    let noise=interleaved_gradient_noise(in.position.xy+vec2(frame.temporal.x*0.618));
    var visible=0.0;
    for(var index=0u;index<16u;index+=1u){
        let travel=(f32(index)+noise)/16.0*reach;
        let h=frame.cascade_clip_from_world[cascade]*vec4(frame.camera_time.xyz+direction*travel,1.0);
        let p=h.xyz/h.w;
        let uv=p.xy*vec2(0.5,-0.5)+vec2(0.5);
        if(all(uv>vec2(0.0))&&all(uv<vec2(1.0))&&p.z>0.0&&p.z<1.0){
            visible+=textureSampleCompareLevel(shadow_map,shadow_sampler,uv,i32(cascade),p.z-bias);
        }
    }
    let g=0.65;
    let denominator=max(1.0+g*g-2.0*g*dot(direction,frame.light_direction.xyz),0.01);
    let phase=(1.0-g*g)/(12.566371*denominator*sqrt(denominator));
    let scattering=visible/16.0*phase*(1.0-exp(-reach*0.002));
    return vec4(frame.light_colour.rgb*scattering*frame.light_direction.w*frame.grade.w,1.0);
}

@fragment fn effects(in:FullscreenVertexOutput)->@location(0) vec4<f32> {
    let size=vec2<f32>(textureDimensions(depth_texture));
    let uv=(floor(in.uv*size)+vec2(0.5))/size;
    let depth=depth_at(uv);
    if(depth<=0.00001 && frame.atmosphere.x>0.5 && frame.atmosphere.w<0.5){
        let ray=ray_at(uv);
        let sky=textureSampleLevel(sky_texture,linear_sampler,sky_uv(ray),0.0).rgb;
        var clouds=vec4(0.0);
        if((frame.flags.x&FEATURE_VOLUMETRIC_CLOUDS)!=0u){clouds=integrate_clouds(frame,ray,8192.0,uv*size);}
        let transmittance=1.0-clouds.a;
        return vec4(clouds.rgb+sky*transmittance,-max(transmittance,0.0001));
    }
    var ao=1.0;
    var contact=1.0;
    if((frame.flags.x&FEATURE_SSAO)!=0u && depth>0.00001){
        ao=horizon_ao(frame,depth_texture,uv,depth,uv*size);
        contact=screen_contact_shadow(frame,depth_texture,uv,depth,uv*size);
    }
    return vec4(ao,contact,0.0,min(frame.projection.x/max(depth,0.00001),60000.0));
}

// Reject half-resolution neighbours across depth discontinuities.
fn filtered_effects(uv:vec2<f32>,depth:f32)->vec4<f32> {
    let size=vec2<f32>(textureDimensions(effects_texture));
    let coordinate=uv*size-vec2(0.5);
    let base=floor(coordinate);
    let sub=fract(coordinate);
    let distance=frame.projection.x/max(depth,0.00001);
    var sum=vec4(0.0);
    var total=0.0;
    for(var y=0u;y<2u;y+=1u){for(var x=0u;x<2u;x+=1u){
        let coordinate=clamp(vec2<i32>(base)+vec2(i32(x),i32(y)),vec2(0),vec2<i32>(size)-vec2(1));
        let value=textureLoad(effects_texture,coordinate,0);
        let axis=vec2(select(1.0-sub.x,sub.x,x==1u),select(1.0-sub.y,sub.y,y==1u));
        var weight=axis.x*axis.y;
        if(depth<=0.00001){weight*=select(0.0,1.0,value.a<0.0);}
        else{weight*=select(0.0,exp(-abs(value.a-distance)/max(0.1,distance*0.03)),value.a>0.0);}
        sum+=value*weight;total+=weight;
    }}
    if(total<0.0001){
        if(depth<=0.00001){return vec4(textureSampleLevel(sky_texture,linear_sampler,sky_uv(ray_at(uv)),0.0).rgb,-1.0);}
        return vec4(1.0,1.0,0.0,distance);
    }
    return sum/total;
}

@fragment fn composite(in:FullscreenVertexOutput)->@location(0) vec4<f32> {
    let uv=in.uv;
    let depth=depth_at(uv);
    var rgb=textureSampleLevel(source_texture,linear_sampler,uv,0.0).rgb;
    if(frame.temporal.w>0.5){return vec4(rgb,1.0);}
    var reactive=0.0;
    if(depth>0.00001 && (frame.flags.x&FEATURE_WATER)!=0u){
        let size=vec2<i32>(textureDimensions(opaque_texture));
        let pixel=clamp(vec2<i32>(uv*vec2<f32>(size)),vec2(0),size-vec2(1));
        reactive=temporal_transparency_reactivity(rgb,textureLoad(opaque_texture,pixel,0).rgb);
    }
    if(depth>0.00001){
        let world=ao_world_position(frame,uv,depth);
        var horizon=vec3(0.0);
        if(finite_world_haze(frame,world)>0.0){
            horizon=textureSampleLevel(sky_texture,linear_sampler,sky_uv(ray_at(uv)),0.0).rgb;
        }
        rgb=physical_aerial(frame,rgb,world,horizon);
    }
    if(frame.atmosphere.w>0.5){
        var travel=64.0;
        if(depth>0.00001){travel=min(length(ao_world_position(frame,uv,depth)-frame.camera_time.xyz),64.0);}
        rgb=water_transport(rgb,travel,water_ambient_irradiance(frame));
    }else{rgb+=textureSampleLevel(shaft_texture,linear_sampler,uv,0.0).rgb;}
    return vec4(min(max(rgb,vec3(0.0)),vec3(60000.0)),reactive);
}

// The background precedes transparency so rain, glass and water retain their composition.
@fragment fn sky_background(in:FullscreenVertexOutput)->@location(0) vec4<f32> {
    let depth=depth_at(in.uv);
    if(depth>0.00001 || frame.atmosphere.x<0.5 || frame.atmosphere.w>0.5){discard;}
    let ray=ray_at(in.uv);
    let effect=filtered_effects(in.uv,depth);
    let radiance=effect.rgb+(atmosphere_discs(frame,ray)+atmosphere_stars(frame,ray))*clamp(-effect.a,0.0,1.0);
    return vec4(min(max(radiance,vec3(0.0)),vec3(60000.0)),1.0);
}

@fragment fn temporal_resolve(in:FullscreenVertexOutput)->@location(0) vec4<f32> {
    let uv=in.uv;
    let depth=depth_at(uv);
    let size=vec2<i32>(textureDimensions(source_texture));
    let pixel=clamp(vec2<i32>(uv*vec2<f32>(size)),vec2(0),size-vec2(1));
    let current_sample=textureLoad(source_texture,pixel,0);
    let current=current_sample.rgb;
    let distance=select(frame.projection.x/max(depth,0.00001),0.0,depth<=0.00001);
    if(frame.temporal.w>0.5){return vec4(current,min(distance,60000.0));}
    var previous_clip:vec4<f32>;
    if(depth<=0.00001){
        let ray=ray_at(uv);
        let effects_size=vec2<i32>(textureDimensions(effects_texture));
        let effect=textureLoad(effects_texture,clamp(vec2<i32>(uv*vec2<f32>(effects_size)),vec2(0),effects_size-vec2(1)),0);
        let transmittance=select(1.0,-effect.a,effect.a<0.0);
        previous_clip=frame.previous_clip_from_world*cloud_history_position(frame,ray,transmittance);
    }
    else{previous_clip=frame.previous_clip_from_world*vec4(ao_world_position(frame,uv,depth),1.0);}
    var previous_uv=(previous_clip.xy/max(previous_clip.w,0.00001))*vec2(0.5,-0.5)+vec2(0.5);
    var expected=select(frame.projection.x*previous_clip.w/max(previous_clip.z,0.00001),0.0,depth<=0.00001);
    let motion=textureLoad(motion_texture,pixel,0);
    if(motion.w>0.5 && depth>0.00001 && current_sample.a<0.03){previous_uv=uv+motion.xy;expected=motion.z;}
    var weight=0.0;
    var history=vec4(current,distance);
    if(frame.temporal.y>0.5 && motion.w>=0.0 && previous_clip.w>0.0 && all(previous_uv>vec2(0.0))&&all(previous_uv<vec2(1.0))){
        history=temporal_history_sample(history_texture,linear_sampler,previous_uv,expected);
        let travel=length((uv-previous_uv)*frame.viewport.xy);
        weight=history.a*mix(0.9,0.55,smoothstep(0.0,16.0,travel));
    }
    var low=vec3(1.0e20);
    var high=vec3(-1.0e20);
    var mean=vec3(0.0);
    var squared=vec3(0.0);
    var count=0.0;
    for(var y=-1;y<=1;y+=1){for(var x=-1;x<=1;x+=1){
        let neighbour=clamp(pixel+vec2(x,y),vec2(0),size-vec2(1));
        let neighbour_depth=textureLoad(depth_texture,neighbour,0);
        let neighbour_distance=select(frame.projection.x/max(neighbour_depth,0.00001),0.0,neighbour_depth<=0.00001);
        if(temporal_depth_matches(neighbour_distance,distance)){
            let sample=temporal_ycocg(textureLoad(source_texture,neighbour,0).rgb);
            low=min(low,sample);high=max(high,sample);mean+=sample;squared+=sample*sample;count+=1.0;
        }
    }}
    mean/=max(count,1.0);
    let deviation=sqrt(max(squared/max(count,1.0)-mean*mean,vec3(0.0)));
    let clipped=temporal_clip(temporal_ycocg(history.rgb),max(low,mean-deviation*1.25),min(high,mean+deviation*1.25));
    let reactive=abs(luminance(history.rgb)-luminance(current))/max(max(luminance(history.rgb),luminance(current)),0.08);
    weight*=1.0-smoothstep(0.12,0.65,reactive);
    weight*=1.0-0.8*clamp(current_sample.a,0.0,1.0);
    let resolved=mix(current,max(temporal_rgb(clipped),vec3(0.0)),weight);
    return vec4(resolved,min(distance,60000.0));
}

fn shadow_debug_colour(uv:vec2<f32>,pixel:vec2<f32>)->vec3<f32> {
    let mode=u32(frame.temporal.w+0.5);
    if(mode>=3u){
        let cascade=mode-3u;
        if(cascade>=frame.flags.y || cascade>=textureNumLayers(shadow_map)){
            return vec3(0.6,0.0,0.6);
        }
        let size=vec2<i32>(textureDimensions(shadow_map));
        let coordinate=clamp(vec2<i32>(uv*vec2<f32>(size)),vec2(0),size-vec2(1));
        let stored=textureLoad(shadow_map,coordinate,i32(cascade),0);
        return vec3(clamp(stored,0.0,1.0));
    }
    let depth=depth_at(uv);
    if(depth<=0.00001){return vec3(0.015);}
    let world=ao_world_position(frame,uv,depth);
    let normal=ao_surface_normal(frame,depth_texture,uv,depth);
    let sample=sun_shadow_sample(frame,shadow_map,shadow_sampler,world,normal,pixel);
    if(mode==2u){return vec3(clamp(sample.x,0.0,1.0));}
    if(sample.y<0.0){return vec3(0.65,0.0,0.65);}
    let colours=array<vec3<f32>,3>(vec3(0.85,0.12,0.08),vec3(0.12,0.8,0.16),vec3(0.12,0.22,0.9));
    return colours[min(u32(sample.y+0.5),2u)];
}

fn display_radiance(radiance:vec3<f32>,exposure_value:f32,debug_view:bool)->vec3<f32> {
    if(debug_view){return radiance;}
    return temporal_display_curve(max(radiance,vec3(0.0))*max(exposure_value,0.001));
}

@fragment fn present(in:FullscreenVertexOutput)->@location(0) vec4<f32> {
    if(frame.temporal.w>0.5){
        return vec4(display_radiance(shadow_debug_colour(in.uv,in.position.xy),1.0,true),1.0);
    }
    let size=vec2<i32>(textureDimensions(source_texture));
    let pixel=clamp(vec2<i32>(in.uv*vec2<f32>(size)),vec2(0),size-vec2(1));
    let centre=textureLoad(source_texture,pixel,0);
    var colour=centre.rgb;
    if(frame.temporal.y>0.5 && centre.a>0.0){
        var neighbours:array<vec3<f32>,4>;
        let offsets=array<vec2<i32>,4>(vec2(-1,0),vec2(1,0),vec2(0,-1),vec2(0,1));
        for(var index=0u;index<4u;index+=1u){
            let value=textureLoad(source_texture,clamp(pixel+offsets[index],vec2(0),size-vec2(1)),0);
            neighbours[index]=select(centre.rgb,value.rgb,temporal_depth_matches(value.a,centre.a));
        }
        colour=temporal_sharpen(centre.rgb,neighbours[0],neighbours[1],neighbours[2],neighbours[3]);
    }
    return vec4(display_radiance(colour,exposure.x,false),1.0);
}
