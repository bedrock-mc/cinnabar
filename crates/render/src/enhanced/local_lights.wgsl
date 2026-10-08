#define_import_path cinnabar::enhanced_local_lights

struct LocalLight { position_radius: vec4<f32>, radiance_shadow: vec4<f32>, shape: vec4<f32>, clip: array<mat4x4<f32>,6>, }
struct LightSources { info: vec4<u32>, lights: array<LocalLight>, }
struct LightTiles { info: vec4<u32>, words: array<u32>, }
@group(2) @binding(10) var<storage,read> local_lights: LightSources;
@group(2) @binding(11) var<storage,read> light_tiles: LightTiles;

fn point_attenuation(distance: f32, radius: f32) -> f32 {
    let window=max(1.0-pow(distance/max(radius,0.01),4.0),0.0);
    return window*window/max(distance*distance,0.25);
}

fn tile_offset(pixel:vec2<f32>)->u32 {
    let size=max(light_tiles.info.xy,vec2(1u));
    let cell=min(vec2<u32>(max(pixel,vec2(0.0)))/max(light_tiles.info.z,1u),size-vec2(1u));
    return (cell.y*size.x+cell.x)*(light_tiles.info.w+1u);
}
fn local_light_count(pixel:vec2<f32>)->u32 {return min(light_tiles.words[tile_offset(pixel)],light_tiles.info.w);}
fn local_light_index(pixel:vec2<f32>,slot:u32)->u32 {return light_tiles.words[tile_offset(pixel)+1u+slot];}
fn capture_light_count()->u32 {return min(local_lights.info.x,arrayLength(&local_lights.lights));}
fn shadowed_light_count()->u32 {return min(local_lights.info.w,4u);}
fn local_light_in_range(index:u32,world:vec3<f32>)->bool {
    let light=local_lights.lights[index];
    let delta=world-light.position_radius.xyz;
    return dot(delta,delta)<light.position_radius.w*light.position_radius.w;
}
fn local_light_direction(index:u32,world:vec3<f32>)->vec3<f32> {
    let delta=local_lights.lights[index].position_radius.xyz-world;
    return delta*inverseSqrt(max(dot(delta,delta),0.0001));
}
fn local_light_incident(index:u32,world:vec3<f32>)->vec3<f32> {
    let light=local_lights.lights[index];
    return light.radiance_shadow.rgb*point_attenuation(distance(world,light.position_radius.xyz),light.position_radius.w);
}
fn local_block_residual(world:vec3<f32>,pixel:vec2<f32>)->f32 {
    if(local_lights.info.x==0u){return 1.0;}
    var coverage=0.0;
    for(var slot=0u;slot<local_light_count(pixel);slot+=1u){
        let light=local_lights.lights[local_light_index(pixel,slot)];
        let delta=world-light.position_radius.xyz;
        let radius_squared=max(light.position_radius.w*light.position_radius.w,0.0001);
        coverage=max(coverage,1.0-smoothstep(radius_squared*0.25,radius_squared,dot(delta,delta)));
    }
    return mix(1.0,0.3,coverage);
}

// Cube captures use world-space membership rather than the parent's screen tiles.
fn capture_block_residual(world:vec3<f32>)->f32 {
    var coverage=0.0;
    for(var index=0u;index<capture_light_count();index+=1u){
        if(!local_light_in_range(index,world)){continue;}
        let light=local_lights.lights[index];
        let delta=world-light.position_radius.xyz;
        let radius_squared=max(light.position_radius.w*light.position_radius.w,0.0001);
        coverage=max(coverage,1.0-smoothstep(radius_squared*0.25,radius_squared,dot(delta,delta)));
    }
    return mix(1.0,0.3,coverage);
}

fn point_shadow_face(direction:vec3<f32>)->u32 {
    let a=abs(direction);
    if(a.x>=a.y && a.x>=a.z){return select(1u,0u,direction.x>=0.0);}
    if(a.y>=a.z){return select(3u,2u,direction.y>=0.0);}
    return select(5u,4u,direction.z>=0.0);
}

// Offset rays select their own cube face; kernels never clamp at a face seam.
fn point_shadow_coordinate(light:LocalLight,ray:vec3<f32>,travel:f32,resolution:u32,map:texture_depth_2d_array)->vec4<f32> {
    let face=point_shadow_face(ray);
    let clip=light.clip[face]*vec4(light.position_radius.xyz+ray*travel,1.0);
    let ndc=clip.xyz/max(clip.w,0.00001);
    let edge=0.5/f32(resolution);
    let face_uv=clamp(ndc.xy*vec2(0.5,-0.5)+vec2(0.5),vec2(edge),vec2(1.0-edge));
    let uv=face_uv*f32(resolution)/vec2<f32>(textureDimensions(map));
    return vec4(uv,ndc.z,light.radiance_shadow.w-1.0+f32(face));
}

fn point_shadow_disk(index:u32,count:u32)->vec2<f32> {
    let angle=f32(index)*2.3999632;
    return vec2(cos(angle),sin(angle))*sqrt((f32(index)+0.5)/f32(count));
}

fn point_shadow_receiver(direction:vec3<f32>,ray:vec3<f32>,normal:vec3<f32>)->f32 {
    let denominator=dot(ray,normal);
    let distance=length(direction);
    if(abs(denominator)<0.04){return distance;}
    return clamp(dot(direction,normal)/denominator,distance*0.5,distance*2.0);
}

fn point_shadow_blocker_distance(light:LocalLight,depth:f32,ray:vec3<f32>)->f32 {
    let near=light.shape.y;
    let far=light.position_radius.w;
    let axial=near*far/max(far-depth*(far-near),0.00001);
    return axial/max(max(abs(ray.x),abs(ray.y)),abs(ray.z));
}

fn point_shadow_texel_ray(light:LocalLight,face:u32,coordinate:vec2<i32>,resolution:u32)->vec3<f32> {
    let clip=light.clip[face];
    let right=vec3(clip[0].x,clip[1].x,clip[2].x);
    let up=vec3(clip[0].y,clip[1].y,clip[2].y);
    let forward=vec3(clip[0].w,clip[1].w,clip[2].w);
    let ndc=(vec2<f32>(coordinate)+vec2(0.5))/f32(resolution)*vec2(2.0,-2.0)+vec2(-1.0,1.0);
    return normalize(forward+right*ndc.x/max(dot(right,right),0.00001)+up*ndc.y/max(dot(up,up),0.00001));
}

// Blocker depth and receiver distance belong to each actual texel ray before interpolation.
fn point_shadow_blocker_footprint(light:LocalLight,resolution:u32,map:texture_depth_2d_array,
    p:vec4<f32>,direction:vec3<f32>,normal:vec3<f32>,bias:f32,face:u32)->vec3<f32> {
    if(p.w<0.0 || p.w>=f32(textureNumLayers(map))){return vec3(0.0);}
    let location=p.xy*vec2<f32>(textureDimensions(map))-vec2(0.5);
    let base=vec2<i32>(floor(location));
    let fraction=fract(location);
    var moments=vec3(0.0);
    for(var y=0;y<2;y+=1){for(var x=0;x<2;x+=1){
        let coordinate=clamp(base+vec2(x,y),vec2(0),vec2(i32(resolution)-1));
        let stored=textureLoad(map,coordinate,i32(p.w),0);
        if(stored>=1.0){continue;}
        let ray=point_shadow_texel_ray(light,face,coordinate,resolution);
        let hit=point_shadow_blocker_distance(light,stored,ray);
        let travel=point_shadow_receiver(direction,ray,normal);
        let separation=max(travel-bias-hit,0.0);
        let confidence=smoothstep(0.001,0.006,separation);
        let axis=vec2(select(1.0-fraction.x,fraction.x,x==1),select(1.0-fraction.y,fraction.y,y==1));
        let weight=axis.x*axis.y*confidence;
        moments+=vec3(hit,separation,1.0)*weight;
    }}
    return moments;
}

fn point_light_visibility_quality(light:LocalLight,resolution:u32,world:vec3<f32>,normal:vec3<f32>,map:texture_depth_2d_array,comparison:sampler_comparison,filter_taps:u32,blocker_taps:u32)->f32 {
    if(light.radiance_shadow.w<0.5){return 1.0;}
    let receiver_normal=select(-normal,normal,dot(normal,light.position_radius.xyz-world)>=0.0);
    let direction=world+receiver_normal*0.006-light.position_radius.xyz;
    let distance=length(direction);
    if(distance<=max(light.shape.y,0.0001) || distance>=light.position_radius.w){return 1.0;}
    let ray=direction/distance;
    let axis=select(vec3(0.0,1.0,0.0),vec3(0.0,0.0,1.0),abs(ray.y)>0.95);
    let tangent=normalize(cross(axis,ray));let bitangent=cross(ray,tangent);
    let facing=max(abs(dot(normal,ray)),0.04);
    let bias=0.004+min(distance/f32(resolution)*sqrt(max(1.0-facing*facing,0.0))/facing,0.02);
    let search=min(light.shape.x/max(min(distance*0.35,2.0),0.25),0.35);
    let filters=select(12u,clamp(filter_taps,1u,32u),filter_taps>0u);
    let blockers=select(8u,clamp(blocker_taps,1u,16u),blocker_taps>0u);
    var blocker=vec3(0.0);
    if(search>0.0 && light.shape.y>0.0){
        for(var index=0u;index<blockers;index+=1u){
            let disk=point_shadow_disk(index,blockers)*search;
            let sample_ray=normalize(ray+tangent*disk.x+bitangent*disk.y);
            let travel=point_shadow_receiver(direction,sample_ray,normal);
            let p=point_shadow_coordinate(light,sample_ray,travel,resolution,map);
            blocker+=point_shadow_blocker_footprint(light,resolution,map,p,direction,normal,bias,point_shadow_face(sample_ray));
        }
    }
    var angular=1.3/f32(resolution);
    if(blocker.z>0.0){
        let separation=blocker.y/max(blocker.z,1.0);
        let hit=blocker.x/max(blocker.z,0.0001);
        angular=max(angular,min(light.shape.x*separation/max(hit*distance,0.0001),0.25));
    }
    var visibility=0.0;
    for(var index=0u;index<filters;index+=1u){
        let disk=point_shadow_disk(index,filters)*angular;
        let sample_ray=normalize(ray+tangent*disk.x+bitangent*disk.y);
        let travel=max(point_shadow_receiver(direction,sample_ray,normal)-bias,light.shape.y);
        let p=point_shadow_coordinate(light,sample_ray,travel,resolution,map);
        if(p.z<=0.0 || p.z>=1.0 || i32(p.w)>=i32(textureNumLayers(map))){visibility+=1.0;}
        else {visibility+=textureSampleCompareLevel(map,comparison,p.xy,i32(p.w),p.z);}
    }
    return visibility/f32(filters);
}

fn point_light_visibility(light:LocalLight,resolution:u32,world:vec3<f32>,normal:vec3<f32>,map:texture_depth_2d_array,comparison:sampler_comparison)->f32 {
    return point_light_visibility_quality(light,resolution,world,normal,map,comparison,0u,0u);
}

fn local_light_visibility_quality(index:u32,world:vec3<f32>,normal:vec3<f32>,map:texture_depth_2d_array,comparison:sampler_comparison,filter_taps:u32,blocker_taps:u32)->f32 {
    if(local_lights.info.z==0u){return 1.0;}
    return point_light_visibility_quality(local_lights.lights[index],local_lights.info.y,world,normal,map,comparison,filter_taps,blocker_taps);
}

fn local_light_visibility(index:u32,world:vec3<f32>,normal:vec3<f32>,map:texture_depth_2d_array,comparison:sampler_comparison)->f32 {
    return local_light_visibility_quality(index,world,normal,map,comparison,0u,0u);
}
