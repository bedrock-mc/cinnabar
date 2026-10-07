#import cinnabar::enhanced_common::EnhancedFrame
#import cinnabar::enhanced_atmosphere::{sky_view_uv,atmosphere_direct_irradiance,cloud_shadow_uv}
#import cinnabar::enhanced_local_lights::{LocalLight,LightSources,point_attenuation}
#import cinnabar::enhanced_radiance::voxel_sky_access
#import cinnabar::enhanced_indirect_trace::{indirect_field,grid_origin,probe_grid_origin,grid_dimensions,grid_contains,grid_cell_index,trace_grid,probe_axes,probe_base,probe_index,probe_radiance_mix}

@group(0) @binding(0) var<uniform> gi_frame:EnhancedFrame;
@group(0) @binding(2) var gi_environment:texture_2d_array<f32>;
@group(0) @binding(3) var gi_sampler:sampler;
@group(0) @binding(4) var<uniform> gi_range:vec4<u32>;
@group(0) @binding(5) var<storage,read> gi_lights:LightSources;
@group(0) @binding(6) var gi_cloud_shadow:texture_2d<f32>;
struct ProbeOrder {indices:array<u32>,}
@group(0) @binding(7) var<storage,read> gi_update_order:ProbeOrder;

const GI_RAYS:u32=32u;
var<workgroup> ray_radiance:array<vec3<f32>,GI_RAYS>;
var<workgroup> ray_sky:array<vec3<f32>,GI_RAYS>;
var<workgroup> ray_direction:array<vec3<f32>,GI_RAYS>;
var<workgroup> ray_distance:array<f32,GI_RAYS>;
var<workgroup> probe_position:vec3<f32>;
var<workgroup> probe_valid:u32;
var<workgroup> solar_irradiance:vec3<f32>;

fn sky_radiance(direction:vec3<f32>)->vec3<f32> {
    return max(textureSampleLevel(gi_environment,gi_sampler,sky_view_uv(direction),i32(textureNumLayers(gi_environment)-2u),0.0).rgb,vec3(0.0));
}
fn sky_visibility(point:vec3<f32>,direction:vec3<f32>,seed:f32)->f32 {
    let ray=trace_grid(point,direction,80.0,seed,false);
    // Unknown directions retain the caller's trusted voxel-skylight bound.
    return select(0.0,1.0,ray.status!=1u);
}
fn cloud_visibility(point:vec3<f32>)->f32 {
    if(gi_frame.cloud_shadow.w<0.5 || point.y>=gi_frame.clouds.y+gi_frame.clouds.z){return 1.0;}
    let uv=cloud_shadow_uv(gi_frame,point);
    if(!all(uv>=vec2(0.0)) || !all(uv<=vec2(1.0))){return 1.0;}
    return textureSampleLevel(gi_cloud_shadow,gi_sampler,uv,0.0).r;
}
fn shade_bounce(point:vec3<f32>,normal:vec3<f32>,albedo:vec3<f32>,seed:f32,raw_sky:f32)->vec3<f32> {
    let offset=point+normal*0.025;
    let sun=gi_frame.light_direction.xyz;
    let sky_bound=voxel_sky_access(clamp(raw_sky,0.0,1.0));
    var irradiance=solar_irradiance*max(dot(normal,sun),0.0)*smoothstep(0.25,0.8,sky_bound)
        *sky_visibility(offset,sun,seed+0.13)*cloud_visibility(point)/3.14159265359;
    let helper=select(vec3(0.0,1.0,0.0),vec3(1.0,0.0,0.0),abs(normal.y)>0.9);
    let tangent=normalize(cross(helper,normal));let bitangent=cross(normal,tangent);
    for(var tap=0u;tap<4u;tap+=1u){
        let angle=f32(tap)*1.57079632679+seed*6.2831853;
        let direction=normal*0.70710678+(tangent*cos(angle)+bitangent*sin(angle))*0.70710678;
        irradiance+=sky_radiance(direction)*sky_visibility(offset,direction,seed+f32(tap)*0.31)*0.25*sky_bound;
    }
    for(var index=0u;index<min(gi_lights.info.x,arrayLength(&gi_lights.lights));index+=1u){
        let light=gi_lights.lights[index];let delta=light.position_radius.xyz-offset;
        let distance=length(delta);
        if(distance<0.05 || distance>=light.position_radius.w){continue;}
        let direction=delta/distance;let cosine=max(dot(normal,direction),0.0);
        if(cosine<=0.0){continue;}
        let ray=trace_grid(offset,direction,max(distance-0.7,0.01),seed+f32(index)*0.17,false);
        if(ray.status==0u){irradiance+=light.radiance_shadow.rgb*point_attenuation(distance,light.position_radius.w)*cosine/3.14159265359;}
    }
    return albedo*irradiance;
}

@compute @workgroup_size(GI_RAYS)
fn update_spatial_irradiance(@builtin(workgroup_id) group:vec3<u32>,@builtin(local_invocation_index) lane:u32){
    let index=gi_update_order.indices[gi_range.x+group.x];
    let size=indirect_field.words[2].xyz;
    let cell=vec3(index%size.x,(index/size.x)%size.y,index/(size.x*size.y));
    let spacing=bitcast<f32>(indirect_field.words[2].w);
    let nominal=probe_grid_origin()+(vec3<f32>(cell)+vec3(0.5))*spacing;
    let seed=fract(dot(floor(nominal/spacing),vec3(0.754877666,0.569840296,0.438579021)));
    if(lane==0u){
        probe_position=nominal;
        probe_valid=0u;
        // A fixed local relocation prevents probes sitting in a solid voxel shell.
        for(var attempt=0u;attempt<7u;attempt+=1u){
            var candidate=probe_position;
            if(attempt>0u){candidate=nominal+probe_axes(attempt-1u);}
            let voxel=vec3<i32>(floor(candidate-grid_origin()));
            if(grid_contains(voxel)){
                let occupancy=indirect_field.words[grid_cell_index(voxel)];
                if(occupancy.x==1u){probe_position=candidate;probe_valid=1u;break;}
            }
        }
        solar_irradiance=atmosphere_direct_irradiance(gi_frame,probe_position);
    }
    workgroupBarrier();
    let z=1.0-2.0*(f32(lane)+0.5)/f32(GI_RAYS);
    let azimuth=f32(lane)*2.39996322973+seed*6.2831853;
    let radius=sqrt(max(1.0-z*z,0.0));
    let direction=vec3(radius*cos(azimuth),z,radius*sin(azimuth));
    ray_direction[lane]=direction;
    ray_radiance[lane]=vec3(0.0);ray_sky[lane]=vec3(0.0);ray_distance[lane]=0.0;
    if(probe_valid!=0u){
        let ray=trace_grid(probe_position,direction,80.0,fract(seed+f32(lane)*0.61803399),false);
        ray_distance[lane]=ray.travel;
        if(ray.status==0u){ray_radiance[lane]=sky_radiance(direction);ray_sky[lane]=ray_radiance[lane];ray_distance[lane]=80.0;}
        // Missing residency uses sky fallback; it never invents a bounce surface.
        if(ray.status==2u){ray_radiance[lane]=sky_radiance(direction);ray_sky[lane]=ray_radiance[lane];ray_distance[lane]=80.0;}
        if(ray.status==1u){ray_radiance[lane]=shade_bounce(ray.position,ray.normal,ray.colour,seed+f32(lane)*0.37,ray.sky);}
    }
    workgroupBarrier();
    if(lane==0u){
        let base=probe_base(probe_index(cell));
        let previous_header=indirect_field.words[base];
        let previous_point=bitcast<vec4<f32>>(previous_header).xyz;
        let reuse=previous_header.w==gi_range.z && all(abs(previous_point-nominal)<=vec3(1.01)) && probe_valid!=0u;
        let history=bitcast<vec4<f32>>(indirect_field.words[base+25u]);
        let previous_mix=probe_radiance_mix(gi_frame.camera_time.w,history.x,history.w);
        for(var face=0u;face<6u;face+=1u){
            let previous=bitcast<vec4<f32>>(indirect_field.words[base+1u+face]);
            let old=bitcast<vec4<f32>>(indirect_field.words[base+13u+face]);
            let previous_sky=bitcast<vec4<f32>>(indirect_field.words[base+7u+face]);
            let old_sky=bitcast<vec4<f32>>(indirect_field.words[base+19u+face]);
            indirect_field.words[base+13u+face]=bitcast<vec4<u32>>(mix(old,previous,previous_mix));
            indirect_field.words[base+19u+face]=bitcast<vec4<u32>>(mix(old_sky,previous_sky,previous_mix));
        }
        indirect_field.words[base+25u]=bitcast<vec4<u32>>(vec4(gi_frame.camera_time.w,0.0,0.0,select(0.0,1.0,reuse)));
        indirect_field.words[base]=vec4(bitcast<vec3<u32>>(probe_position),select(0u,gi_range.z,probe_valid!=0u));
        for(var face=0u;face<6u;face+=1u){
            let axis=probe_axes(face);var colour=vec3(0.0);var sky_colour=vec3(0.0);var mean=0.0;var second=0.0;var weights=0.0;
            for(var ray=0u;ray<GI_RAYS;ray+=1u){
                let weight=max(dot(axis,ray_direction[ray]),0.0);
                colour+=ray_radiance[ray]*weight;mean+=ray_distance[ray]*weight;
                sky_colour+=ray_sky[ray]*weight;
                second+=ray_distance[ray]*ray_distance[ray]*weight;weights+=weight;
            }
            indirect_field.words[base+1u+face]=bitcast<vec4<u32>>(vec4(colour*(4.0/f32(GI_RAYS)),mean/max(weights,0.001)));
            indirect_field.words[base+7u+face]=bitcast<vec4<u32>>(vec4(second/max(weights,0.001),sky_colour*(4.0/f32(GI_RAYS))));
        }
    }
}
