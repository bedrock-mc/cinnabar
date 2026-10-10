#define_import_path cinnabar::enhanced_indirect
#import cinnabar::enhanced_indirect_trace::{indirect_field,probe_grid_origin,grid_segment_visibility,probe_axes,probe_base,probe_index,probe_radiance_mix}

fn probe_lobe(base:u32,direction:vec3<f32>,moments:bool,seconds:f32)->vec4<f32> {
    let squared=direction*direction;
    let offset=select(1u,7u,moments);
    let x=select(1u,0u,direction.x>=0.0);
    let y=select(3u,2u,direction.y>=0.0);
    let z=select(5u,4u,direction.z>=0.0);
    var current=bitcast<vec4<f32>>(indirect_field.words[base+offset+x])*squared.x
        + bitcast<vec4<f32>>(indirect_field.words[base+offset+y])*squared.y
        + bitcast<vec4<f32>>(indirect_field.words[base+offset+z])*squared.z;
    let history=bitcast<vec4<f32>>(indirect_field.words[base+25u]);
    let history_mix=probe_radiance_mix(seconds,history.x,history.w);
    if(history_mix<0.99999){
        let old_offset=select(13u,19u,moments);
        let previous=bitcast<vec4<f32>>(indirect_field.words[base+old_offset+x])*squared.x
            + bitcast<vec4<f32>>(indirect_field.words[base+old_offset+y])*squared.y
            + bitcast<vec4<f32>>(indirect_field.words[base+old_offset+z])*squared.z;
        let blended=mix(previous,current,history_mix);
        if(moments){current=vec4(current.x,blended.yzw);}
        else{current=vec4(blended.rgb,current.w);}
    }
    return current;
}

// E/pi is interpolated only from the receiver's connected air volume.
fn spatial_indirect(world:vec3<f32>,normal:vec3<f32>,sky_access:f32,seconds:f32)->vec4<f32> {
    if(indirect_field.words[3].y==0u){return vec4(0.0);}
    let size=indirect_field.words[2].xyz;
    let spacing=bitcast<f32>(indirect_field.words[2].w);
    let location=(world-probe_grid_origin())/spacing-vec3(0.5);
    if(!all(location>=vec3(0.0)) || !all(location<vec3<f32>(size)-vec3(1.0))){return vec4(0.0);}
    let base_cell=vec3<u32>(floor(location));let fraction=fract(location);
    let receiver=world+normal*0.08;
    var total=vec3(0.0);var weight_sum=0.0;var initialized=0.0;
    for(var corner=0u;corner<8u;corner+=1u){
        let shift=vec3(corner&1u,(corner>>1u)&1u,(corner>>2u)&1u);
        let cell=base_cell+shift;
        let base=probe_base(probe_index(cell));
        let header=indirect_field.words[base];
        if(header.w!=indirect_field.words[3].x){continue;}
        let point=bitcast<vec4<f32>>(header).xyz;
        let nominal=probe_grid_origin()+(vec3<f32>(cell)+vec3(0.5))*spacing;
        if(any(abs(point-nominal)>vec3(1.01))){continue;}
        let delta=receiver-point;let distance=length(delta);
        let direction=delta/max(distance,0.001);
        let trilinear=select(vec3(1.0)-fraction,fraction,shift!=vec3(0u));
        var weight=trilinear.x*trilinear.y*trilinear.z;
        initialized+=weight;
        // Smooth backface weighting still admits probes beside a flat receiver.
        weight*=pow(clamp(dot(-direction,normal)*0.5+0.5,0.05,1.0),2.0);
        if(weight<0.001){continue;}
        let mean=probe_lobe(base,direction,false,seconds).w;
        let second=probe_lobe(base,direction,true,seconds).x;
        let variance=max(second-mean*mean,0.05);
        let excess=max(distance-mean,0.0);
        var visibility=variance/(variance+excess*excess);
        visibility*=visibility*visibility;
        if(visibility<0.01){continue;}
        visibility*=grid_segment_visibility(receiver,point);
        weight*=visibility;
        let radiance=probe_lobe(base,normal,false,seconds).rgb;
        let sky=probe_lobe(base,normal,true,seconds).yzw;
        total+=(max(radiance-sky,vec3(0.0))+sky*clamp(sky_access,0.0,1.0))*weight;
        weight_sum+=weight;
    }
    if(weight_sum<=0.00001){return vec4(0.0);}
    let edge=min(min(min(location.x,location.y),location.z),min(min(f32(size.x-1u)-location.x,f32(size.y-1u)-location.y),f32(size.z-1u)-location.z));
    return vec4(total/weight_sum,smoothstep(0.0,0.75,edge)*smoothstep(0.0,0.85,initialized));
}
