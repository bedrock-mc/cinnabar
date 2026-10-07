#define_import_path cinnabar::enhanced_indirect_trace

struct IndirectBuffer { words:array<vec4<u32>>, }
const GI_HISTORY_SECONDS:f32=0.12;
fn probe_radiance_mix(seconds:f32,updated:f32,valid:f32)->f32 {
    return select(1.0,smoothstep(0.0,GI_HISTORY_SECONDS,seconds-updated),valid>0.5 && seconds>=updated);
}
#ifdef INDIRECT_COMPUTE
@group(0) @binding(1) var<storage,read_write> indirect_field:IndirectBuffer;
#else
@group(2) @binding(12) var<storage,read> indirect_field:IndirectBuffer;
#endif

struct GridTrace { position:vec3<f32>, normal:vec3<f32>, colour:vec3<f32>, travel:f32, status:u32, sky:f32, }

fn grid_origin()->vec3<f32> { return bitcast<vec4<f32>>(indirect_field.words[0]).xyz; }
fn probe_grid_origin()->vec3<f32> { return grid_origin()+vec3(bitcast<f32>(indirect_field.words[0].w)*0.5); }
fn grid_dimensions()->vec3<u32> { return indirect_field.words[1].xyz; }
fn grid_cell_index(cell:vec3<i32>)->u32 { let size=grid_dimensions();return indirect_field.words[3].w+(u32(cell.z)*size.y+u32(cell.y))*size.x+u32(cell.x); }
fn grid_contains(cell:vec3<i32>)->bool {return all(cell>=vec3(0)) && all(cell<vec3<i32>(grid_dimensions()));}
fn grid_colour(packed:u32)->vec3<f32> {return vec3(f32(packed&255u),f32((packed>>8u)&255u),f32((packed>>16u)&255u))/255.0;}

// Integer traversal stops at unknown resident data rather than treating it as air.
fn trace_grid(start:vec3<f32>,direction:vec3<f32>,limit:f32,seed:f32,skip_first:bool)->GridTrace {
    let local=start-grid_origin();
    var cell=vec3<i32>(floor(local));
    let step=select(vec3(-1),vec3(1),direction>=vec3(0.0));
    let inverse=1.0/max(abs(direction),vec3(1.0e-6));
    let boundary=vec3<f32>(cell)+select(vec3(0.0),vec3(1.0),direction>=vec3(0.0));
    var next=abs(boundary-local)*inverse;
    var travel=0.0;
    var normal=-direction;
    for(var iteration=0u;iteration<96u;iteration+=1u){
        if(travel>=limit || !grid_contains(cell)){return GridTrace(start+direction*min(travel,limit),normal,vec3(0.0),min(travel,limit),0u,1.0);}
        let value=indirect_field.words[grid_cell_index(cell)];
        if((value.x&1u)==0u){return GridTrace(start+direction*travel,normal,vec3(0.0),travel,2u,0.0);}
        if((value.x&2u)!=0u && !(skip_first && iteration==0u)){
            let opacity=clamp(bitcast<f32>(value.z),0.0,1.0);
            let test=fract(seed+f32(iteration)*0.61803399);
            if(opacity>=0.999 || test<opacity){return GridTrace(start+direction*travel,normal,grid_colour(value.y),travel,1u,bitcast<f32>(value.w));}
        }
        var axis=0u;
        if(next.y<next.x){axis=1u;}
        if(next.z<next[axis]){axis=2u;}
        travel=next[axis];next[axis]+=inverse[axis];
        cell[axis]+=step[axis];normal=vec3(0.0);normal[axis]=-f32(step[axis]);
    }
    return GridTrace(start+direction*min(travel,limit),normal,vec3(0.0),min(travel,limit),2u,0.0);
}

// Interpolation uses deterministic opacity transport, avoiding temporal leak noise.
fn grid_segment_visibility(start:vec3<f32>,end:vec3<f32>)->f32 {
    let delta=end-start;let limit=length(delta);
    if(limit<0.01){return 1.0;}
    let direction=delta/limit;let local=start-grid_origin();
    var cell=vec3<i32>(floor(local));
    let step=select(vec3(-1),vec3(1),direction>=vec3(0.0));
    let inverse=1.0/max(abs(direction),vec3(1.0e-6));
    var next=abs(vec3<f32>(cell)+select(vec3(0.0),vec3(1.0),direction>=vec3(0.0))-local)*inverse;
    var travel=0.0;var visibility=1.0;
    for(var iteration=0u;iteration<24u;iteration+=1u){
        if(travel>=limit){return visibility;}
        if(!grid_contains(cell)){return 0.0;}
        let value=indirect_field.words[grid_cell_index(cell)];
        if((value.x&1u)==0u){return 0.0;}
        if(iteration>0u && (value.x&2u)!=0u){visibility*=1.0-clamp(bitcast<f32>(value.z),0.0,1.0);if(visibility<0.01){return 0.0;}}
        var axis=0u;if(next.y<next.x){axis=1u;}if(next.z<next[axis]){axis=2u;}
        travel=next[axis];next[axis]+=inverse[axis];cell[axis]+=step[axis];
    }
    return 0.0;
}

fn probe_axes(face:u32)->vec3<f32> {
    var result=vec3(0.0);result[face/2u]=select(-1.0,1.0,(face&1u)==0u);return result;
}
fn probe_base(index:u32)->u32 {return indirect_field.words[1].w+index*indirect_field.words[3].z;}
// Absolute probe cells retain their storage slots as the camera grid scrolls.
fn probe_storage_index(world_cell:vec3<i32>,dimensions:vec3<u32>)->u32 {
    let size=vec3<i32>(dimensions);
    let wrapped=vec3<u32>(((world_cell%size)+size)%size);
    return(wrapped.z*dimensions.y+wrapped.y)*dimensions.x+wrapped.x;
}
fn probe_index(cell:vec3<u32>)->u32 {
    let spacing=bitcast<f32>(indirect_field.words[2].w);
    let world_cell=vec3<i32>(floor(grid_origin()/spacing))+vec3<i32>(cell);
    return probe_storage_index(world_cell,indirect_field.words[2].xyz);
}
