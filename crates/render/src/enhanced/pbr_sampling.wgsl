#ifdef ENHANCED
#import cinnabar::enhanced_pbr::{MaterialBasis,material_basis,material_normal,material_specular_aa,blend_material_sample,material_relief_weight,material_relief_ray,PBR_REF_LABPBR}

fn authored_material_ref(texture_ref:u32)->u32 {
    return enhanced_texture_refs[(texture_ref>>31u)*2048u+(texture_ref&0x7ffu)];
}
// Metal identifiers and the porosity/SSS branch are categorical channels.
fn sample_lab_categories(page:texture_2d_array<f32>,sampled:vec4<f32>,uv:vec2<f32>,layer:i32,dx:vec2<f32>,dy:vec2<f32>)->vec4<f32> {
    let base=vec2<f32>(textureDimensions(page));
    let footprint=max(length(dx*base),length(dy*base));
    let level=i32(clamp(floor(log2(max(footprint,1.0))+0.5),0.0,f32(textureNumLevels(page)-1u)));
    let size=vec2<f32>(textureDimensions(page,level));
    let pixel=vec2<i32>(min(floor(fract(uv)*size),size-vec2(1.0)));
    let exact=textureLoad(page,pixel,layer,level);
    return vec4(exact.x,sampled.yz,exact.w);
}
fn sample_pbr_texture(normal_map:bool,texture_ref:u32,uv:vec2<f32>,dx:vec2<f32>,dy:vec2<f32>)->vec4<f32> {
    let authored_ref=authored_material_ref(texture_ref);
    if (authored_ref!=0xffffffffu) {
        let layer=i32(authored_ref&0x7ffu);
        if (normal_map) {
            if ((authored_ref>>31u)==0u) {return textureSampleGrad(enhanced_normal_page_0,enhanced_sampler,uv,layer,dx,dy);}
            return textureSampleGrad(enhanced_normal_page_1,enhanced_sampler,uv,layer,dx,dy);
        }
        if ((authored_ref>>31u)==0u) {
            let sampled=textureSampleGrad(enhanced_mer_page_0,enhanced_sampler,uv,layer,dx,dy);
            if ((authored_ref&PBR_REF_LABPBR)!=0u) {return sample_lab_categories(enhanced_mer_page_0,sampled,uv,layer,dx,dy);}
            return sampled;
        }
        let sampled=textureSampleGrad(enhanced_mer_page_1,enhanced_sampler,uv,layer,dx,dy);
        if ((authored_ref&PBR_REF_LABPBR)!=0u) {return sample_lab_categories(enhanced_mer_page_1,sampled,uv,layer,dx,dy);}
        return sampled;
    }
    return select(vec4(0.0,0.0,0.8,0.0),vec4(0.5,0.5,1.0,0.5),normal_map);
}

fn parallax_material_uv(texture_ref:u32,uv:vec2<f32>,dx:vec2<f32>,dy:vec2<f32>,view:vec3<f32>,basis:MaterialBasis,distance:f32,allowed:bool)->vec2<f32> {
    let weight=material_relief_weight(authored_material_ref(texture_ref),allowed,distance,max(length(dx),length(dy)),abs(dot(view,basis.normal)));
    if (weight<=0.001) {return uv;}
    let ray=material_relief_ray(view,basis,weight);
    let layers=12u;
    var depth=0.0;var sampled_uv=uv;var previous_uv=uv;var previous_difference=0.0;
    for (var step=0u;step<layers;step+=1u) {
        let height=sample_pbr_texture(true,texture_ref,sampled_uv,dx,dy).a;
        let difference=depth-(1.0-height);
        if (difference>=0.0) {
            let fraction=clamp(-previous_difference/max(difference-previous_difference,1.0e-6),0.0,1.0);
            return mix(previous_uv,sampled_uv,fraction);
        }
        previous_uv=sampled_uv;previous_difference=difference;
        depth=f32(step+1u)/f32(layers); sampled_uv=uv+ray*depth;
    }
    return sampled_uv;
}
#endif

#ifdef ENHANCED
fn parallax_direct_visibility(texture_ref:u32,uv:vec2<f32>,dx:vec2<f32>,dy:vec2<f32>,direction:vec3<f32>,basis:MaterialBasis,distance:f32,allowed:bool)->f32 {
    let weight=material_relief_weight(authored_material_ref(texture_ref),allowed,distance,max(length(dx),length(dy)),abs(dot(direction,basis.normal)));
    if(weight<=0.001){return 1.0;}
    let above=dot(direction,basis.normal);
    if(above<=0.0){return 0.0;}
    let ray=-material_relief_ray(direction,basis,weight);
    let height=sample_pbr_texture(true,texture_ref,uv,dx,dy).a;
    var visibility=1.0;
    for(var step=1u;step<=5u;step+=1u){
        let rise=(1.0-height)*f32(step)/5.0;
        let blocker=sample_pbr_texture(true,texture_ref,uv+ray*rise,dx,dy).a;
        visibility=min(visibility,1.0-smoothstep(0.01,0.06,blocker-height-rise));
    }
    return mix(1.0,visibility,weight);
}
#endif
