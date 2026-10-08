#define_import_path cinnabar::enhanced_sun_shadow_temporal

#import cinnabar::enhanced_common::decode_geometric_normal

struct SunReceiverPlane {
    uv:vec2<f32>,
    depth:f32,
    gradient:vec2<f32>,
}

// Projected planes have affine device depth, even when the receiver is viewed obliquely.
fn sun_shadow_receiver_plane(clip:mat4x4<f32>,world:vec3<f32>,normal:vec3<f32>)->SunReceiverPlane {
    let reference=select(vec3(0.0,1.0,0.0),vec3(1.0,0.0,0.0),abs(normal.y)>0.9);
    let tangent=normalize(cross(reference,normal));
    let bitangent=cross(normal,tangent);
    let centre=clip*vec4(world,1.0);
    let a=clip*vec4(tangent,0.0);
    let b=clip*vec4(bitangent,0.0);
    let ndc=centre.xyz/max(centre.w,0.00001);
    let plane=cross(a.xyz*centre.w-centre.xyz*a.w,b.xyz*centre.w-centre.xyz*b.w);
    var gradient=vec2(0.0);
    if(abs(plane.z)>0.00000001){gradient=-plane.xy/plane.z*vec2(2.0,-2.0);}
    return SunReceiverPlane(ndc.xy*vec2(0.5,-0.5)+vec2(0.5),ndc.z,gradient);
}

fn shadow_receiver_depth_tolerance(expected:f32)->f32 {
    // Retain half-float rounding tolerance without accepting another nearby surface.
    return max(0.006,expected*0.001);
}

fn shadow_receiver_weight(depth:f32,encoded_normal:vec2<f32>,normal:vec3<f32>,expected:f32)->f32 {
    if(depth<=0.0 || expected<=0.0){return 0.0;}
    let tolerance=shadow_receiver_depth_tolerance(expected);
    let depth_weight=1.0-smoothstep(tolerance*0.5,tolerance,abs(depth-expected));
    let normal_weight=smoothstep(0.9999,0.99999,abs(dot(normalize(normal),decode_geometric_normal(encoded_normal))));
    return depth_weight*normal_weight;
}

fn sun_shadow_receiver_weight(record:vec4<f32>,normal:vec3<f32>,expected:f32)->f32 {
    return shadow_receiver_weight(record.a,record.gb,normal,expected);
}

fn sun_shadow_record_uv(pixel:vec2<i32>,size:vec2<i32>,viewport:vec2<f32>)->vec2<f32> {
    return (floor((vec2<f32>(pixel)+vec2(0.5))/vec2<f32>(size)*viewport)+vec2(0.5))/viewport;
}

struct ShadowReceiverFootprint {
    base:vec2<i32>,
    fraction:vec2<f32>,
}

fn shadow_receiver_footprint(uv:vec2<f32>,size:vec2<i32>,viewport:vec2<f32>)->ShadowReceiverFootprint {
    let coordinate=uv*vec2<f32>(size)-vec2(0.5);
    var base=vec2<i32>(floor(coordinate));
    let initial=sun_shadow_record_uv(base,size,viewport);
    let next=sun_shadow_record_uv(base+vec2(1),size,viewport);
    base-=select(vec2(0),vec2(1),uv<initial);
    base+=select(vec2(0),vec2(1),uv>=next);
    let lower=sun_shadow_record_uv(base,size,viewport);
    let upper=sun_shadow_record_uv(base+vec2(1),size,viewport);
    return ShadowReceiverFootprint(base,clamp((uv-lower)/max(upper-lower,vec2(0.000001)),vec2(0.0),vec2(1.0)));
}

// Match the full-resolution depth texel actually owned by each half-resolution record.
fn sun_shadow_history_sample(history:texture_2d<f32>,plane:SunReceiverPlane,normal:vec3<f32>,near:f32,viewport:vec2<f32>)->vec2<f32> {
    let size=vec2<i32>(textureDimensions(history));
    let footprint=shadow_receiver_footprint(plane.uv,size,viewport);
    var visibility=0.0;var total=0.0;
    for(var y=0;y<2;y+=1){for(var x=0;x<2;x+=1){
        let pixel=footprint.base+vec2(x,y);
        if(any(pixel<vec2(0)) || any(pixel>=size)){continue;}
        let uv=sun_shadow_record_uv(pixel,size,viewport);
        let depth=plane.depth+dot(plane.gradient,uv-plane.uv);
        if(depth<=0.0){continue;}
        let expected=near/depth;
        let record=textureLoad(history,pixel,0);
        let axis=vec2(select(1.0-footprint.fraction.x,footprint.fraction.x,x==1),select(1.0-footprint.fraction.y,footprint.fraction.y,y==1));
        let weight=axis.x*axis.y*sun_shadow_receiver_weight(record,normal,expected);
        visibility+=clamp(record.r,0.0,1.0)*weight;total+=weight;
    }}
    return vec2(visibility/max(total,0.00001),clamp(total,0.0,1.0));
}

struct LocalShadowSample {
    visibility:vec4<f32>,
    confidence:f32,
}

fn local_shadow_history_sample(history:texture_2d<f32>,metadata:texture_2d<f32>,plane:SunReceiverPlane,normal:vec3<f32>,near:f32,viewport:vec2<f32>)->LocalShadowSample {
    let size=vec2<i32>(textureDimensions(history));
    let footprint=shadow_receiver_footprint(plane.uv,size,viewport);
    var visibility=vec4(0.0);var total=0.0;
    for(var y=0;y<2;y+=1){for(var x=0;x<2;x+=1){
        let pixel=footprint.base+vec2(x,y);
        if(any(pixel<vec2(0)) || any(pixel>=size)){continue;}
        let record=textureLoad(metadata,pixel,0);
        if(record.x<0.5){continue;}
        let uv=sun_shadow_record_uv(pixel,size,viewport);
        let depth=plane.depth+dot(plane.gradient,uv-plane.uv);
        if(depth<=0.0){continue;}
        let axis=vec2(select(1.0-footprint.fraction.x,footprint.fraction.x,x==1),select(1.0-footprint.fraction.y,footprint.fraction.y,y==1));
        let weight=axis.x*axis.y*shadow_receiver_weight(record.y,record.zw,normal,near/depth);
        visibility+=clamp(textureLoad(history,pixel,0),vec4(0.0),vec4(1.0))*weight;total+=weight;
    }}
    return LocalShadowSample(visibility/max(total,0.00001),clamp(total,0.0,1.0));
}
