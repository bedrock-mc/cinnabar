#import bevy_render::view::View
#import bevy_render::globals::Globals
#import cinnabar::lighting::{tint_to_linear, tint_to_gamma}

struct Shape {
    transform: mat4x4<f32>,
    color: vec4<f32>,
    data: vec4<f32>,
    flags: vec4<u32>,
    lifetime: vec4<f32>,
}
struct Actor { position: vec3<f32>, valid: u32, }
struct TextRecord { rect: vec4<f32>, uv: vec4<f32>, color: vec4<f32>, flags: vec4<u32>, }
struct Frame { epoch: f32, dimension: u32, render_distance: f32, padding: u32, }
@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read> shapes: array<Shape>;
@group(0) @binding(2) var<uniform> frame: Frame;
@group(0) @binding(3) var<uniform> globals: Globals;
@group(0) @binding(4) var<storage, read> actors: array<Actor>;
@group(0) @binding(5) var<storage, read> texts: array<TextRecord>;
@group(0) @binding(6) var atlas: texture_2d<f32>;
@group(0) @binding(7) var atlas_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) @interpolate(flat) textured: u32,
    @location(3) @interpolate(flat) backface: u32,
}

// Attachment indices address one compact actor entry shared by every attached shape.
fn anchor(shape: Shape) -> vec3<f32> {
    var result=shape.transform[3].xyz;
    if shape.flags.z!=0xffffffffu { result+=actors[shape.flags.z].position; }
    return result;
}

// Server-owned slots remain allocated when dimension or distance hides their geometry.
fn visible(shape: Shape, origin: vec3<f32>) -> bool {
    if shape.flags.x==0u { return false; }
    if shape.flags.w==ARROW_KIND_VALUEu && shape.data.z*shape.data.z<ARROW_MIN_LENGTH_SQUARED_VALUE { return false; }
    if shape.flags.y!=ALL_DIMENSIONS_VALUEu && shape.flags.y!=frame.dimension { return false; }
    if shape.lifetime.x>=0.0 && frame.epoch+globals.time>=shape.lifetime.x { return false; }
    if shape.flags.z!=0xffffffffu && actors[shape.flags.z].valid==0u { return false; }
    var position=origin;
    if shape.flags.w==BOX_KIND_VALUEu { position-=0.5*(shape.transform[0]+shape.transform[1]+shape.transform[2]).xyz; }
    let delta=position-view.world_position;
    let range=select(frame.render_distance,shape.data.w,shape.data.w>=0.0);
    if !(dot(delta,delta)<range*range) { return false; }
    return true;
}

// Unit meshes use packet-time transforms; actor offsets remain independently updateable.
@vertex
fn shape_vertex(@location(0) vertex: vec4<f32>, @builtin(instance_index) index: u32) -> VertexOutput {
    let shape=shapes[index];
    let origin=anchor(shape);
    var point=vertex.xyz;
    if shape.flags.w==ARROW_KIND_VALUEu {
        point=vec3(vertex.xy*shape.data.y,vertex.z*shape.data.z);
        if vertex.w!=0.0 { point.z=shape.data.z-shape.data.x; }
    }
    var output:VertexOutput;
    var world=origin+(shape.transform*vec4(point,0.0)).xyz;
    if vertex.w<0.0 { world=vec3(0.0); }
    output.position=view.clip_from_world*vec4(world,1.0);
    if !visible(shape,origin) { output.position=vec4(2.0,2.0,2.0,1.0); }
    output.color=select(shape.color,vec4(0.0),vertex.w<0.0);
    output.uv=vec2(0.0);
    output.textured=2u;
    output.backface=1u;
    return output;
}

// Billboard angles preserve the shared nametag approximation.
fn native_acos(value:f32) -> f32 {
    return (ACOS_CUBIC_VALUE*value*value*value-ACOS_LINEAR_VALUE*value)+1.5707963267948966;
}

// Facing uses the unlifted anchor, while multiline lift remains independent of scale.
@vertex
fn text_vertex(@builtin(vertex_index) vertex:u32,@builtin(instance_index) index:u32) -> VertexOutput {
    let record=texts[index];
    let shape=shapes[record.flags.x];
    let origin=anchor(shape);
    let at=array<vec2<f32>,6>(vec2(0.0,0.0),vec2(1.0,0.0),vec2(1.0,1.0),vec2(0.0,0.0),vec2(1.0,1.0),vec2(0.0,1.0))[vertex];
    let local=mix(record.rect.xy,record.rect.zw,at);
    var right=shape.transform[0].xyz;
    var down=shape.transform[1].xyz;
    if (record.flags.z&8u)==0u {
        let direction=view.world_position-origin;
        let x=select(direction.x,HORIZONTAL_ZERO_VALUE,direction.x==0.0);
        let z=select(direction.z,HORIZONTAL_ZERO_VALUE,direction.z==0.0);
        let horizontal=sqrt(x*x+z*z);
        let yaw=native_acos(-z/horizontal)*select(1.0,-1.0,x>0.0);
        let pitch_dot=(direction.x*(x/horizontal)+direction.z*(z/horizontal))/length(direction);
        let pitch=native_acos(pitch_dot)*select(-1.0,1.0,direction.y>0.0);
        let scale=length(right)*select(1.0,-1.0,dot(cross(right,down),shape.transform[2].xyz)<0.0);
        right=vec3(-cos(yaw),0.0,sin(yaw))*scale;
        down=vec3(-sin(yaw)*sin(pitch),-cos(pitch),-cos(yaw)*sin(pitch))*scale;
    } else {
        right=-right;
        down=-down;
    }
    let textured=record.uv.z>=0.0;
    var show=visible(shape,origin)&&record.flags.y!=0u;
#ifdef TEXT_DEPTH
    show=show&&(record.flags.z&1u)!=0u;
#else
    show=show&&(record.flags.z&1u)==0u;
#endif
#ifdef TEXT_GLYPH
    show=show&&textured;
#else
    show=show&&!textured;
#endif
    var output:VertexOutput;
    output.position=view.clip_from_world*vec4(origin+vec3(0.0,bitcast<f32>(record.flags.w),0.0)+(right*local.x+down*local.y)*TEXT_SCALE_VALUE,1.0);
    if !show { output.position=vec4(2.0,2.0,2.0,1.0); }
    output.color=record.color;
    if textured { output.color*=shape.color; }
    output.uv=mix(record.uv.xy,record.uv.zw,at);
    output.textured=u32(textured);
    output.backface=select(record.flags.z&2u,record.flags.z&4u,textured);
    return output;
}

// Background and glyph backfaces have separate controls; geometry never alpha-blends.
@fragment
fn shape_fragment(input:VertexOutput,@builtin(front_facing) front:bool) -> @location(0) vec4<f32> {
    if !front&&input.backface==0u { discard; }
    var color=input.color;
    if input.textured==1u {
        var texel=textureSample(atlas,atlas_sampler,input.uv);
#ifdef TEXT_ALPHA_TEST
        if texel.a<0.5 { discard; }
#endif
#ifdef SHAPE_GAMMA
        texel=tint_to_gamma(texel);
#endif
        color*=texel;
    }
#ifndef SHAPE_GAMMA
    if input.textured==2u { return tint_to_linear(color); }
#endif
    if input.textured==2u { return color; }
    if color.a<=0.0 { discard; }
    return color;
}
