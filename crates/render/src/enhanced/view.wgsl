#define_import_path cinnabar::enhanced_view

#import cinnabar::lighting::lit_colour
#import cinnabar::enhanced_atmosphere::{environment_sky, sky_view_uv, atmosphere_direct_irradiance,cloud_shadow_uv}
#import cinnabar::enhanced_environment::{environment_coordinate, environment_specular_weight, environment_diffuse_weight}
#import cinnabar::enhanced_radiance::{block_illumination, compose_surface_lighting, voxel_sky_access, diffuse_indirect}
#import cinnabar::enhanced_indirect::spatial_indirect
#import cinnabar::enhanced_pbr::material_response
#import cinnabar::enhanced_shadow::sun_shadow_sample
#import cinnabar::enhanced_sun_shadow_temporal::{sun_shadow_receiver_plane,sun_shadow_history_sample,local_shadow_history_sample}
#import cinnabar::enhanced_actor_motion::stationary_receiver_motion
#import cinnabar::enhanced_local_lights::{local_light_count, local_light_index, local_light_direction, local_light_incident, local_light_visibility_quality, local_block_residual, capture_light_count, local_light_in_range, capture_block_residual, shadowed_light_count}
#import cinnabar::enhanced_water::{water_fresnel, ripple_normal, trace_reflection,
    water_refraction, water_caustic, water_transport, water_screen_reflection_weight,
    scene_depth_at, scene_world, water_surface_offset}

#import cinnabar::enhanced_common::{
    EnhancedFrame, CLASS_EMISSION_MASK, CLASS_LAVA, CLASS_LEAVES, CLASS_PLANT, CLASS_WATER,
    FEATURE_PBR, FEATURE_SHADOWS, FEATURE_WATER, FEATURE_WAVING, FEATURE_SSAO, interleaved_gradient_noise, wave_offset,
}

// Main-pass bind group appended as group 2 only on Enhanced pipelines.
@group(2) @binding(0) var<uniform> enhanced_frame: EnhancedFrame;
@group(2) @binding(1) var enhanced_shadow_map: texture_depth_2d_array;
@group(2) @binding(2) var enhanced_shadow_sampler: sampler_comparison;
@group(2) @binding(3) var enhanced_material_classes: texture_2d<u32>;
@group(2) @binding(4) var enhanced_scene_colour: texture_2d<f32>;
@group(2) @binding(5) var enhanced_scene_depth: texture_depth_2d;
@group(2) @binding(6) var enhanced_linear_sampler: sampler;
@group(2) @binding(7) var enhanced_environment: texture_2d_array<f32>;
@group(2) @binding(8) var enhanced_cloud_shadows: texture_2d<f32>;
@group(2) @binding(9) var enhanced_surface_visibility: texture_2d<f32>;
@group(2) @binding(16) var enhanced_local_shadow_visibility: texture_2d<f32>;
@group(2) @binding(17) var enhanced_scene_motion: texture_2d<f32>;
@group(2) @binding(18) var<uniform> enhanced_shadow_frame: EnhancedFrame;
@group(2) @binding(19) var enhanced_local_shadow_metadata: texture_2d<f32>;
@group(2) @binding(20) var enhanced_point_shadow_map: texture_depth_2d_array;
@group(2) @binding(21) var enhanced_sun_shadow_visibility: texture_2d<f32>;

const EMISSIVE_GAIN: f32 = 2.4;
const MAX_EMISSIVE_RADIANCE: f32 = 12.0;
const PI: f32 = 3.14159265359;

fn exact_local_visibility(index:u32,world:vec3<f32>,normal:vec3<f32>)->f32 {
    return local_light_visibility_quality(index,world,normal,enhanced_point_shadow_map,
        enhanced_shadow_sampler,u32(max(enhanced_shadow_frame.quality.w,0.0)),u32(max(enhanced_shadow_frame.quality.z,0.0)));
}

fn safe_direction(value: vec3<f32>, fallback: vec3<f32>) -> vec3<f32> {
    let squared = dot(value, value);
    if (!(squared > 1.0e-8 && squared < 1.0e20)) { return fallback; }
    return value * inverseSqrt(squared);
}

fn smooth_local_visibility(index:u32,world:vec3<f32>,normal:vec3<f32>,pixel:vec2<f32>)->f32 {
    if(index<shadowed_light_count() && enhanced_frame.projection.w>0.5 && enhanced_frame.probe.w>=0.0){
        let previous=enhanced_frame.previous_clip_from_world*vec4(world,1.0);
        let old_uv=previous.xy/max(previous.w,0.00001)*vec2(0.5,-0.5)+vec2(0.5);
        let motion_size=vec2<i32>(textureDimensions(enhanced_scene_motion));
        let motion=textureLoad(enhanced_scene_motion,clamp(vec2<i32>(pixel),vec2(0),motion_size-vec2(1)),0);
        if(previous.w<=0.0 || !stationary_receiver_motion(motion,old_uv-pixel*enhanced_frame.viewport.zw,enhanced_frame.viewport.xy)){
            return exact_local_visibility(index,world,normal);
        }
        let plane=sun_shadow_receiver_plane(enhanced_frame.clip_from_world,world,normal);
        let resolved=local_shadow_history_sample(enhanced_local_shadow_visibility,enhanced_local_shadow_metadata,
            plane,normal,enhanced_frame.projection.x,enhanced_frame.viewport.xy);
        if(resolved.confidence>0.999){return resolved.visibility[index];}
        return mix(exact_local_visibility(index,world,normal),resolved.visibility[index],resolved.confidence);
    }
    return exact_local_visibility(index,world,normal);
}

// Read the palette-derived class, defaulting unknown IDs to ordinary surfaces.
fn material_class(material_id: u32) -> u32 {
    let size = textureDimensions(enhanced_material_classes);
    if (material_id >= size.x * size.y) { return 0u; }
    return textureLoad(enhanced_material_classes,
        vec2<i32>(i32(material_id % size.x), i32(material_id / size.x)), 0).r;
}

fn enhanced_light_direction()->vec3<f32>{return enhanced_frame.light_direction.xyz;}
fn enhanced_materials_enabled()->bool{return (enhanced_frame.flags.x&FEATURE_PBR)!=0u;}

fn enhanced_physical_atmosphere() -> bool {
    return enhanced_frame.atmosphere.x > 0.5;
}

fn enhanced_cloud_visibility(world: vec3<f32>) -> f32 {
    if (enhanced_shadow_frame.cloud_shadow.w < 0.5
        || world.y >= enhanced_shadow_frame.clouds.y + enhanced_shadow_frame.clouds.z) { return 1.0; }
    let uv = cloud_shadow_uv(enhanced_shadow_frame,world);
    if (!all(uv >= vec2(0.0)) || !all(uv <= vec2(1.0))) { return 1.0; }
    return clamp(textureSampleLevel(enhanced_cloud_shadows, enhanced_linear_sampler, uv, 0.0).r, 0.0, 1.0);
}

// Return foliage displacement separately so camera-relative projection retains small offsets.
fn foliage_displacement(world: vec3<f32>, surface_class: u32, weight: f32) -> vec3<f32> {
    if ((enhanced_frame.flags.x & FEATURE_WAVING) == 0u) {
        return vec3(0.0);
    }
    return wave_offset(
        world,
        surface_class,
        weight,
        enhanced_frame.camera_time.w,
        enhanced_frame.ambient_colour.w,
    );
}

// Return the shared water-edge displacement without adding it to a large world coordinate.
fn water_displacement(world: vec3<f32>, top_surface: bool) -> vec3<f32> {
    if ((enhanced_frame.flags.x & FEATURE_WAVING) == 0u || !top_surface) {
        return vec3(0.0);
    }
    return vec3(0.0, water_surface_offset(world, enhanced_frame.camera_time.w), 0.0);
}

fn shadow_visibility(world: vec3<f32>, normal: vec3<f32>, pixel: vec2<f32>) -> f32 {
    var resolved=vec2(0.0);
    if((enhanced_frame.flags.x&FEATURE_SHADOWS)!=0u && enhanced_frame.projection.w>0.5
        && enhanced_frame.probe.w>=0.0 && enhanced_frame.temporal.w<0.5){
        let previous=enhanced_frame.previous_clip_from_world*vec4(world,1.0);
        let old_uv=previous.xy/max(previous.w,0.00001)*vec2(0.5,-0.5)+vec2(0.5);
        let size=vec2<i32>(textureDimensions(enhanced_scene_motion));
        let motion=textureLoad(enhanced_scene_motion,clamp(vec2<i32>(pixel),vec2(0),size-vec2(1)),0);
        if(previous.w>0.0 && stationary_receiver_motion(motion,old_uv-pixel*enhanced_frame.viewport.zw,enhanced_frame.viewport.xy)){
            let plane=sun_shadow_receiver_plane(enhanced_frame.clip_from_world,world,normal);
            resolved=sun_shadow_history_sample(enhanced_sun_shadow_visibility,plane,normal,enhanced_frame.projection.x,enhanced_frame.viewport.xy);
            if(resolved.y>0.999){return resolved.x;}
        }
    }
    let current=sun_shadow_sample(enhanced_shadow_frame,enhanced_shadow_map,enhanced_shadow_sampler,world,normal,pixel).x;
    return mix(current,resolved.x,resolved.y);
}

// Boost bright texels of materials whose block states all emit light.
fn emissive_light(albedo: vec3<f32>, surface_class: u32) -> vec3<f32> {
    let level = f32(surface_class & CLASS_EMISSION_MASK) / 15.0;
    if (level <= 0.0) {
        return vec3(0.0);
    }
    // Bright texels glow; dark detail (torch sticks, lamp frames) stays matte.
    let mask = smoothstep(0.3, 0.85, max(albedo.r, max(albedo.g, albedo.b)));
    return min(albedo * level * mask * EMISSIVE_GAIN, vec3(MAX_EMISSIVE_RADIANCE));
}

fn fresnel_schlick(cos_theta: f32, f0: vec3<f32>) -> vec3<f32> {
    return f0 + (vec3(1.0) - f0) * pow(1.0 - clamp(cos_theta, 0.0, 1.0), 5.0);
}



fn distribution_ggx(n_dot_h: f32, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let cosine_squared = n_dot_h * n_dot_h;
    let denominator = (1.0 - cosine_squared) + cosine_squared * a2;
    return a2 / max(PI * denominator * denominator, 1.0e-12);
}

fn geometry_schlick_ggx(n_dot_x: f32, roughness: f32) -> f32 {
    let k = (roughness + 1.0) * (roughness + 1.0) / 8.0;
    return n_dot_x / max(n_dot_x * (1.0 - k) + k, 1.0e-5);
}

fn geometry_smith(n_dot_v: f32, n_dot_l: f32, roughness: f32) -> f32 {
    return geometry_schlick_ggx(n_dot_v, roughness)
        * geometry_schlick_ggx(n_dot_l, roughness);
}

// Cook-Torrance direct lighting with a metallic workflow. This is deliberately
// bounded to one directional light so Enhanced remains a forward renderer.
fn cook_torrance(
    albedo: vec3<f32>,
    normal: vec3<f32>,
    view: vec3<f32>,
    light: vec3<f32>,
    roughness: f32,
    metallic: f32,
    dielectric_f0: f32,
) -> vec3<f32> {
    return cook_authored(albedo,normal,view,light,roughness,metallic,mix(vec3(dielectric_f0),albedo,metallic));
}
fn cook_authored(albedo:vec3<f32>,normal:vec3<f32>,view:vec3<f32>,light:vec3<f32>,roughness:f32,metallic:f32,f0:vec3<f32>)->vec3<f32> {
    let n_dot_l = max(dot(normal, light), 0.0);
    let n_dot_v = max(dot(normal, view), 0.0);
    if (n_dot_l <= 0.0 || n_dot_v <= 0.0) {
        return vec3(0.0);
    }
    let halfway = safe_direction(view + light, normal);
    let n_dot_h = max(dot(normal, halfway), 0.0);
    let v_dot_h = max(dot(view, halfway), 0.0);
    let fresnel = fresnel_schlick(v_dot_h, f0);
    let distribution = distribution_ggx(n_dot_h, roughness);
    let visibility = geometry_smith(n_dot_v, n_dot_l, roughness);
    let specular = distribution * visibility * fresnel
        / max(4.0 * n_dot_v * n_dot_l, 1.0e-5);
    let diffuse = (vec3(1.0) - fresnel) * (1.0 - metallic) * albedo / PI;
    return (diffuse + specular) * n_dot_l;
}

fn reflection_sky_filtered(direction: vec3<f32>, roughness: f32) -> vec3<f32> {
    let layer = i32(textureNumLayers(enhanced_environment) - 1u);
    let mip = clamp(roughness, 0.0, 1.0) * f32(textureNumLevels(enhanced_environment) - 1u);
    let cached = textureSampleLevel(enhanced_environment, enhanced_linear_sampler,
        sky_view_uv(direction), layer, mip);
    if (cached.a > 0.5) { return max(cached.rgb, vec3(0.0)); }
    return environment_sky(enhanced_frame, direction);
}

fn reflection_sky(direction: vec3<f32>) -> vec3<f32> {
    return reflection_sky_filtered(direction, 0.0);
}

fn diffuse_sky(normal: vec3<f32>) -> vec3<f32> {
    let layer = i32(textureNumLayers(enhanced_environment) - 3u);
    let mip = f32((textureNumLevels(enhanced_environment) - 1u) / 2u);
    let cached = textureSampleLevel(enhanced_environment, enhanced_linear_sampler,
        sky_view_uv(normal), layer, mip);
    if (cached.a > 0.5) { return max(cached.rgb, vec3(0.0)); }
    return reflection_sky_filtered(normal, 1.0);
}

// The spatial field includes visibility and one authored-colour diffuse bounce.
fn surface_indirect(world: vec3<f32>, normal: vec3<f32>, sky_light: f32, block: vec3<f32>, pixel: vec2<f32>) -> vec3<f32> {
    let sky = diffuse_sky(normal) * sky_light;
    var indirect = sky;
    let field = spatial_indirect(world, normal, sky_light, enhanced_frame.camera_time.w);
    indirect = mix(sky, field.rgb, field.a);
    var residual=1.0;
    if(enhanced_frame.probe.w<0.0){residual=capture_block_residual(world);}
    else {residual=local_block_residual(world,pixel);}
    return indirect + max(block, vec3(0.0)) * residual;
}

fn reflection_environment(world: vec3<f32>, direction: vec3<f32>, roughness: f32, sky_light: f32) -> vec3<f32> {
    if (enhanced_frame.probe.w <= 0.0) { return reflection_sky_filtered(direction, roughness) * sky_light; }
    let radius = enhanced_frame.probe.w;
    let relative = world - enhanced_frame.probe.xyz;
    let box_distance = max(abs(relative.x), max(abs(relative.y), abs(relative.z))) / radius;
    if (box_distance >= 1.0) { return reflection_sky_filtered(direction, roughness) * sky_light; }
    // Local surfaces intersect a box around the capture point to reduce sliding.
    let bounds = select(vec3(-radius), vec3(radius), direction >= vec3(0.0));
    let divisor = select(vec3(-1.0), vec3(1.0), direction >= vec3(0.0))
        * max(abs(direction), vec3(1.0e-5));
    let intersections = (bounds - relative) / divisor;
    let travel = max(min(intersections.x, min(intersections.y, intersections.z)), 0.0);
    let corrected = safe_direction(relative + direction * travel, direction);
    let coordinate = environment_coordinate(corrected);
    let mip = clamp(roughness, 0.0, 1.0) * f32(textureNumLevels(enhanced_environment) - 1u);
    let captured = textureSampleLevel(enhanced_environment, enhanced_linear_sampler,
        coordinate.xy, i32(coordinate.z), mip);
    let confidence = clamp(captured.a, 0.0, 1.0)
        * (1.0 - smoothstep(0.7, 1.0, box_distance));
    if (confidence >= 0.999) { return max(captured.rgb, vec3(0.0)); }
    let sky = reflection_sky_filtered(direction, roughness) * sky_light;
    return mix(sky, max(captured.rgb, vec3(0.0)), confidence);
}

// A leaf divides incident energy between reflection and thin-sheet transmission.
fn foliage_light(albedo: vec3<f32>, normal: vec3<f32>, view: vec3<f32>, light: vec3<f32>) -> vec3<f32> {
    let facing = dot(normal, light);
    let reflected = max(facing, 0.0) * 0.72 * albedo / PI;
    let forward_scatter = 0.35 + 0.65 * pow(max(dot(view, -light), 0.0), 8.0);
    let transmitted = max(-facing, 0.0) * 0.24 * forward_scatter
        * clamp(albedo, vec3(0.0), vec3(1.0)) / PI;
    return reflected + transmitted;
}

fn local_material_lighting(albedo:vec3<f32>,normal:vec3<f32>,view:vec3<f32>,world:vec3<f32>,pixel:vec2<f32>,roughness:f32,metallic:f32,f0:f32,foliage:bool,block:vec3<f32>)->vec3<f32> {
    return local_authored_lighting(albedo,normal,normal,view,world,pixel,roughness,metallic,mix(vec3(f0),albedo,metallic),foliage,block);
}

fn local_surface_lighting(albedo:vec3<f32>,normal:vec3<f32>,view:vec3<f32>,world:vec3<f32>,pixel:vec2<f32>,roughness:f32,metallic:f32,foliage:bool,block:vec3<f32>)->vec3<f32>{
    return local_material_lighting(albedo,normal,view,world,pixel,roughness,metallic,0.04,foliage,block);
}

fn local_authored_lighting(albedo:vec3<f32>,normal:vec3<f32>,shadow_normal:vec3<f32>,view:vec3<f32>,world:vec3<f32>,pixel:vec2<f32>,roughness:f32,metallic:f32,fzero:vec3<f32>,foliage:bool,block:vec3<f32>)->vec3<f32> {
    var result=vec3(0.0);
    let capture=enhanced_frame.probe.w<0.0;
    var count=0u;
    if(capture){count=capture_light_count();}
    else {count=local_light_count(pixel);}
    let block_access=clamp(max(block.r,max(block.g,block.b))*10.0,0.0,1.0);
    for (var slot=0u;slot<count;slot+=1u) {
        var index=slot;
        if(!capture){index=local_light_index(pixel,slot);}
        if(!local_light_in_range(index,world)){continue;}
        let light=local_light_direction(index,world);
        let incident=local_light_incident(index,world);
        if(max(incident.r,max(incident.g,incident.b))<0.0001){continue;}
        let visibility=smooth_local_visibility(index,world,shadow_normal,pixel);
        let brdf=cook_authored(albedo,normal,view,light,roughness,metallic,fzero);
        result+=select(brdf,foliage_light(albedo,normal,view,light),foliage)*incident*visibility*block_access;
    }
    return result;
}

// Occlusion affects incident light; emission keeps its authored radiance.
fn shade_surface(albedo:vec3<f32>,normal:vec3<f32>,world:vec3<f32>,pixel:vec2<f32>,lighting:vec3<f32>,sky_light:f32,ambient_occlusion:f32,surface_class:u32,pbr_normal:vec3<f32>,pbr_mer:vec3<f32>)->vec3<f32> {
    return shade_material(albedo,normal,world,pixel,lighting,sky_light,ambient_occlusion,surface_class,pbr_normal,vec4(pbr_mer,0.0),0u,1.0);
}

fn shade_material(
    albedo: vec3<f32>,
    normal: vec3<f32>,
    world: vec3<f32>,
    pixel: vec2<f32>,
    lighting: vec3<f32>,
    sky_light: f32,
    ambient_occlusion: f32,
    surface_class: u32,
    pbr_normal: vec3<f32>,
    pbr_mer: vec4<f32>,
    material_flags: u32,
    material_direct_visibility: f32,
) -> vec3<f32> {
    if (enhanced_frame.projection.w < 0.5 && enhanced_frame.probe.w >= 0.0) {
        let incident = diffuse_indirect(enhanced_frame, normal, sky_light, lighting);
        let direct = atmosphere_direct_irradiance(enhanced_frame, world)
            * max(dot(normal, enhanced_frame.light_direction.xyz), 0.0) * sky_light / PI;
        return albedo * (incident * ambient_occlusion + direct)
            + emissive_light(albedo, surface_class);
    }
    let light = enhanced_frame.light_direction.xyz;
    let foliage = (surface_class & (CLASS_LEAVES | CLASS_PLANT)) != 0u;
    let facing = dot(normal, light);
    let diffuse = select(max(facing, 0.0), abs(facing), foliage);
    let sky_gate = smoothstep(0.25, 0.8, sky_light);
    var shadow = 0.0;
    if (diffuse > 0.0 && sky_gate > 0.0) {
        let bias_normal = select(normal, -normal, foliage && facing < 0.0);
        shadow = shadow_visibility(world, bias_normal, pixel) * enhanced_cloud_visibility(world);
    }
    let view = safe_direction(enhanced_frame.camera_time.xyz - world, normal);
    let irradiance = atmosphere_direct_irradiance(enhanced_frame, world);
    let direct = irradiance * diffuse * shadow * sky_gate;
    let ao = clamp(ambient_occlusion, 0.0, 1.0);
    let visibility = surface_visibility(pixel);
    let classic_direct = select(albedo * direct, foliage_light(albedo, normal, view, light)
        * irradiance * shadow * sky_gate, foliage);
    if ((enhanced_frame.flags.x & FEATURE_PBR) == 0u) {
        let ambient = surface_indirect(world, normal, sky_light, lighting, pixel) * ao;
        return min(compose_surface_lighting(albedo * ambient, classic_direct, emissive_light(albedo, surface_class), visibility)
            + local_surface_lighting(albedo,normal,view,world,pixel,0.85,0.0,foliage,lighting), vec3(24.0));
    }

    let mapped_normal = safe_direction(pbr_normal, normal);
    let response=material_response(pbr_mer,material_flags,albedo);
    let wetness=enhanced_frame.ambient_colour.w*sky_gate*max(normal.y,0.0);
    let surface_albedo=albedo*(1.0-0.35*response.porosity*wetness);
    let roughness=mix(response.roughness,min(response.roughness,0.24+response.porosity*0.3),wetness);
    let metallic=response.metallic;
    let f0=response.fzero;
    let direct_brdf = cook_authored(
        surface_albedo,
        mapped_normal,
        view,
        light,
        roughness,
        metallic,
        f0,
    ) * irradiance * shadow * sky_gate;
    let foliage_direct = irradiance * foliage_light(albedo, mapped_normal, view, light) * shadow * sky_gate;
    let transmission=irradiance*foliage_light(surface_albedo,mapped_normal,view,light)*shadow*sky_gate;
    let pbr_direct = select(mix(direct_brdf,transmission,response.subsurface*0.6),foliage_direct,foliage)*material_direct_visibility;
    let reflected = reflect(-view, mapped_normal);
    let environment = reflection_environment(world, reflected, roughness, sky_light);
    let n_dot_v = abs(dot(mapped_normal, view));
    let specular_weight = environment_specular_weight(f0, roughness, n_dot_v);
    let diffuse_weight = environment_diffuse_weight(f0, roughness, n_dot_v, metallic);
    var diffuse_radiance = surface_indirect(world, mapped_normal, sky_light, lighting, pixel);
    if (foliage) {
        diffuse_radiance = 0.72 * diffuse_radiance
            + 0.24 * surface_indirect(world, -mapped_normal, sky_light, lighting, pixel);
    }
    let indirect = surface_albedo * diffuse_radiance * diffuse_weight * ao
        + environment * specular_weight * ao;
    let texture_emission = min(albedo * response.emission * 4.5, vec3(MAX_EMISSIVE_RADIANCE));
    return min(
        compose_surface_lighting(indirect, pbr_direct, max(emissive_light(albedo, surface_class), texture_emission), visibility)
            + local_authored_lighting(surface_albedo,mapped_normal,normal,view,world,pixel,roughness,metallic,f0,foliage,lighting),
        vec3(32.0),
    );
}

fn surface_visibility_weight(distance: f32, sample_distance: f32) -> f32 {
    return exp(-abs(sample_distance - distance) / max(0.025, min(0.04, distance * 0.002)));
}

// Match half-resolution samples to the current prepass depth at silhouette edges.
fn surface_visibility(pixel: vec2<f32>) -> vec2<f32> {
    if ((enhanced_frame.flags.x & FEATURE_SSAO) == 0u || enhanced_frame.projection.w < 0.5 || enhanced_frame.probe.w < 0.0) {
        return vec2(1.0);
    }
    let uv = pixel * enhanced_frame.viewport.zw;
    let depth_size = vec2<i32>(textureDimensions(enhanced_scene_depth));
    let depth = textureLoad(enhanced_scene_depth, clamp(vec2<i32>(pixel), vec2(0), depth_size - vec2(1)), 0);
    if (depth <= 0.00001) { return vec2(1.0); }
    let distance = enhanced_frame.projection.x / depth;
    let size = vec2<f32>(textureDimensions(enhanced_surface_visibility));
    let coordinate = uv * size - vec2(0.5);
    let base = floor(coordinate);
    let fraction = fract(coordinate);
    var sum = vec2(0.0);
    var total = 0.0;
    for (var y = 0u; y < 2u; y += 1u) {
        for (var x = 0u; x < 2u; x += 1u) {
            let offset = vec2(f32(x), f32(y));
            let p = clamp(vec2<i32>(base + offset), vec2(0), vec2<i32>(size) - vec2(1));
            let sample = textureLoad(enhanced_surface_visibility, p, 0);
            let bilinear = select(vec2(1.0) - fraction, fraction, vec2<bool>(x == 1u, y == 1u));
            let weight = bilinear.x * bilinear.y * surface_visibility_weight(distance, sample.a);
            if (sample.a > 0.0) { sum += sample.rg * weight; total += weight; }
        }
    }
    if (total <= 0.00001) { return vec2(1.0); }
    return clamp(sum / total, vec2(0.0), vec2(1.0));
}

fn shade_actor_surface(albedo: vec3<f32>, normal: vec3<f32>, world: vec3<f32>, pixel: vec2<f32>, packed_light: u32) -> vec3<f32> {
    if ((packed_light & 0x80000000u) == 0u) { return albedo; }
    let n = safe_direction(normal, vec3(0.0, 1.0, 0.0));
    let sky = sky_illumination(packed_light);
    if (enhanced_frame.projection.w < 0.5 && enhanced_frame.probe.w >= 0.0) {
        return shade_surface(albedo, n, world, pixel, block_illumination(packed_light), sky,
            1.0, 0u, n, vec3(0.0, 0.0, 0.85));
    }
    let light = enhanced_frame.light_direction.xyz;
    let view = safe_direction(enhanced_frame.camera_time.xyz - world, n);
    let visibility = shadow_visibility(world, n, pixel) * enhanced_cloud_visibility(world);
    let direct = cook_torrance(albedo, n, view, light, 0.85, 0.0, 0.04)
        * atmosphere_direct_irradiance(enhanced_frame, world) * visibility * smoothstep(0.25, 0.8, sky);
    let coefficient = environment_specular_weight(vec3(0.04), 0.85, max(dot(n, view), 0.0));
    let indirect = albedo * surface_indirect(world, n, sky, block_illumination(packed_light), pixel) * (vec3(1.0) - coefficient)
        + reflection_environment(world, reflect(-view, n), 0.85, sky) * coefficient;
    return compose_surface_lighting(indirect, direct, vec3(0.0), surface_visibility(pixel))
        + local_surface_lighting(albedo,n,view,world,pixel,0.85,0.0,false,block_illumination(packed_light));
}

// The opaque snapshot is already composited here; full alpha avoids blending it twice.
fn shade_water(
    base: vec3<f32>,
    alpha: f32,
    face_normal: vec3<f32>,
    world: vec3<f32>,
    frag: vec4<f32>,
    lighting: vec3<f32>,
    sky_light: f32,
    ambient_occlusion: f32,
    sky_zenith: vec3<f32>,
    sky_horizon: vec3<f32>,
    footprint: f32,
) -> vec4<f32> {
    let to_camera = enhanced_frame.camera_time.xyz - world;
    let above = dot(to_camera, face_normal) > 0.0;
    if ((enhanced_frame.flags.x & FEATURE_WATER) == 0u || enhanced_frame.probe.w < 0.0
        || enhanced_frame.projection.w < 0.5) {
        let lit = shade_surface(base, face_normal, world, frag.xy, lighting, sky_light,
            ambient_occlusion, 0u, face_normal, vec3(0.0, 0.0, 0.82));
        return vec4(lit, alpha);
    }
    let distance_to_camera = length(to_camera);
    let eye = to_camera / max(distance_to_camera, 1.0e-4);
    var surface_normal = safe_direction(face_normal, vec3(0.0, 1.0, 0.0));
    if (face_normal.y > 0.5) {
        let ripple = ripple_normal(world.xz, enhanced_frame.camera_time.w, footprint,
            (enhanced_frame.flags.x & FEATURE_WAVING) != 0u);
        surface_normal = safe_direction(mix(ripple, surface_normal,
            smoothstep(24.0, 72.0, distance_to_camera)), surface_normal);
    }
    let normal = select(-surface_normal, surface_normal, above);
    let n_dot_v = max(dot(normal, eye), 0.0);
    let eta = select(1.333, 1.0 / 1.333, above);
    let refracted = refract(-eye, normal, eta);
    let total_reflection = dot(refracted, refracted) < 1.0e-6;
    let fresnel = select(water_fresnel(n_dot_v, eta), 1.0, total_reflection);
    let reflected = reflect(-eye, normal);
    let roughness = mix(0.16, 0.25, smoothstep(24.0, 96.0, distance_to_camera));
    let environment = reflection_environment(world, reflected, roughness, sky_light);
    let noise = interleaved_gradient_noise(frag.xy);
    let screen_weight = water_screen_reflection_weight(fresnel, distance_to_camera);
    var traced = vec4(0.0);
    if (screen_weight > 0.001) {
        traced = trace_reflection(enhanced_frame, enhanced_scene_colour, enhanced_scene_depth,
            enhanced_linear_sampler, world + normal * 0.035, reflected, noise, roughness);
    }
    let reflection = mix(environment, traced.rgb, traced.a * screen_weight);
    let sky_gate = smoothstep(0.25, 0.8, sky_light);
    let sun_visibility = shadow_visibility(world, face_normal, frag.xy)
        * enhanced_cloud_visibility(world) * sky_gate;
    let water_f0 = water_fresnel(1.0, eta);
    let glint = cook_torrance(vec3(0.0), normal, eye, enhanced_frame.light_direction.xyz,
        roughness, 0.0, water_f0) * atmosphere_direct_irradiance(enhanced_frame, world) * sun_visibility
        + local_material_lighting(vec3(0.0),normal,eye,world,frag.xy,roughness,0.0,water_f0,false,lighting);
    if (total_reflection) { return vec4(reflection + glint, 1.0); }

    let uv = frag.xy * enhanced_frame.viewport.zw;
    let background = water_refraction(enhanced_frame, enhanced_scene_colour, enhanced_scene_depth, enhanced_linear_sampler, world, face_normal, refracted, uv, above);
    // The post pass handles extinction between an underwater camera and this surface.
    let path = select(0.0, background.a, above);
    let medium_light = surface_indirect(world, face_normal, sky_light, lighting, frag.xy);
    var transmitted = water_transport(background.rgb, path, medium_light);
    let original_depth = scene_depth_at(enhanced_scene_depth, uv);
    var vertical_depth = 48.0;
    if (original_depth > 0.0) {
        let bed = scene_world(enhanced_frame, uv, original_depth);
        vertical_depth = max(world.y - bed.y, 0.0);
        if (above && face_normal.y > 0.5) {
            transmitted *= 1.0 + water_caustic(enhanced_frame, bed, path, sun_visibility
                * enhanced_frame.light_direction.w);
        }
    }
    let shore_band = (1.0 - smoothstep(0.18, 0.65, vertical_depth))
        * smoothstep(0.015, 0.08, vertical_depth);
    let foam_phase = sin(world.x * 5.7 + enhanced_frame.camera_time.w * 0.7)
        * sin(world.z * 6.3 - enhanced_frame.camera_time.w * 0.8);
    let foam = select(0.0, shore_band * smoothstep(0.1, 0.65, foam_phase) * 0.65, above && face_normal.y > 0.5);
    let foam_colour = vec3(0.8, 0.88, 0.9) * (medium_light
        + atmosphere_direct_irradiance(enhanced_frame, world) * sun_visibility / PI);
    let radiance = (1.0 - fresnel) * transmitted + fresnel * reflection + glint;
    return vec4(mix(radiance, foam_colour, foam), 1.0);
}

// Raw sky access gates directional sunlight independently of block illumination.
fn sky_illumination(sample: u32) -> f32 {
    let value = f32((sample >> 4u) & 15u) / 15.0;
    return voxel_sky_access(value);
}
