#define_import_path cinnabar::enhanced_actor_motion

struct MotionInstance {
    world_row_x:vec4<f32>,
    world_row_y:vec4<f32>,
    world_row_z:vec4<f32>,
    metadata:vec4<u32>,
}
struct MotionBone {
    bone_row_x:vec4<f32>,
    bone_row_y:vec4<f32>,
    bone_row_z:vec4<f32>,
}
@group(3) @binding(0) var<storage,read> motion_instances:array<MotionInstance>;
@group(3) @binding(1) var<storage,read> motion_bones:array<MotionBone>;

fn submitted_actor_position(instance_index:u32,bone_index:u32,local:vec3<f32>)->vec4<f32> {
    let instance=motion_instances[instance_index];
    if(instance.metadata.z==0u || bone_index>=instance.metadata.y){return vec4(0.0);}
    let bone=motion_bones[instance.metadata.x+bone_index];
    let point=vec4(local,1.0);
    let posed=vec4(dot(bone.bone_row_x,point),dot(bone.bone_row_y,point),dot(bone.bone_row_z,point),1.0);
    return vec4(dot(instance.world_row_x,posed),dot(instance.world_row_y,posed),dot(instance.world_row_z,posed),1.0);
}

// Store small UV displacements so RGBA16F preserves subpixel motion at high resolutions.
fn submitted_surface_motion(current_clip:vec4<f32>,previous_clip:vec4<f32>,valid:bool)->vec4<f32> {
    if(!valid || current_clip.w<=0.0 || previous_clip.w<=0.0){return vec4(0.0,0.0,0.0,-1.0);}
    let old_uv=previous_clip.xy/previous_clip.w*vec2(0.5,-0.5)+vec2(0.5);
    let current_uv=current_clip.xy/current_clip.w*vec2(0.5,-0.5)+vec2(0.5);
    return vec4(old_uv-current_uv,previous_clip.w,1.0);
}

// Camera reprojection includes jitter; only residual surface motion rejects lamp history.
fn stationary_receiver_motion(motion:vec4<f32>,camera_delta:vec2<f32>,viewport:vec2<f32>)->bool {
    return motion.w>0.5 && length((motion.xy-camera_delta)*viewport)<=0.1;
}
