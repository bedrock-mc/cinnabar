#define_import_path cinnabar::enhanced_temporal

fn temporal_luminance(rgb:vec3<f32>)->f32 {return dot(rgb,vec3(0.2126,0.7152,0.0722));}
fn temporal_ycocg(rgb:vec3<f32>)->vec3<f32>{return vec3(dot(rgb,vec3(0.25,0.5,0.25)),rgb.r*0.5-rgb.b*0.5,-rgb.r*0.25+rgb.g*0.5-rgb.b*0.25);}
fn temporal_rgb(v:vec3<f32>)->vec3<f32>{return vec3(v.x+v.y-v.z,v.x+v.z,v.x-v.y-v.z);}

fn temporal_depth_matches(stored:f32,expected:f32)->bool {
    if(expected<=0.0){return stored<=0.0;}
    return stored>0.0 && abs(stored-expected)<max(0.06,expected*0.015);
}

fn temporal_transparency_reactivity(current:vec3<f32>,opaque:vec3<f32>)->f32 {
    let relative=abs(current-opaque)/max(max(abs(current),abs(opaque)),vec3(0.04));
    return smoothstep(0.03,0.25,max(max(relative.x,relative.y),relative.z));
}

fn catmull_weights(f:f32)->vec4<f32> {
    let squared=f*f;
    let cubed=squared*f;
    return vec4(-0.5*f+squared-0.5*cubed,1.0-2.5*squared+1.5*cubed,0.5*f+2.0*squared-1.5*cubed,-0.5*squared+0.5*cubed);
}

// Nine bilinear taps reconstruct a separable cubic without accumulating linear-filter softness.
fn temporal_history_sample(history:texture_2d<f32>,linear:sampler,uv:vec2<f32>,expected:f32)->vec4<f32> {
    let size=vec2<f32>(textureDimensions(history));
    let coordinate=uv*size-vec2(0.5);
    let base=floor(coordinate);
    let wx=catmull_weights(fract(coordinate.x));
    let wy=catmull_weights(fract(coordinate.y));
    let sx=vec3(base.x-1.0,base.x+wx.z/(wx.y+wx.z),base.x+2.0);
    let sy=vec3(base.y-1.0,base.y+wy.z/(wy.y+wy.z),base.y+2.0);
    let weights_x=vec3(wx.x,wx.y+wx.z,wx.w);
    let weights_y=vec3(wy.x,wy.y+wy.z,wy.w);
    var sum=vec3(0.0);
    var total=0.0;
    var central_valid=false;
    for(var y=0u;y<3u;y+=1u){for(var x=0u;x<3u;x+=1u){
        let sample_uv=(vec2(sx[x],sy[y])+vec2(0.5))/size;
        let value=textureSampleLevel(history,linear,sample_uv,0.0);
        let valid=temporal_depth_matches(value.a,expected);
        if(x==1u && y==1u){central_valid=valid;}
        if(valid){
            let weight=weights_x[x]*weights_y[y];
            sum+=value.rgb*weight;
            total+=weight;
        }
    }}
    if(!central_valid || total<0.1){return vec4(0.0);}
    return vec4(max(sum/total,vec3(0.0)),1.0);
}

// Clip the history ray into the neighbourhood box without rotating its chroma direction.
fn temporal_clip(history:vec3<f32>,low:vec3<f32>,high:vec3<f32>)->vec3<f32> {
    let centre=(low+high)*0.5;
    let extent=max((high-low)*0.5,vec3(0.00001));
    let ray=history-centre;
    let outside=max(max(abs(ray.x)/extent.x,abs(ray.y)/extent.y),abs(ray.z)/extent.z);
    return centre+ray/max(outside,1.0);
}

fn temporal_sharpen(centre:vec3<f32>,a:vec3<f32>,b:vec3<f32>,c:vec3<f32>,d:vec3<f32>)->vec3<f32> {
    let low=min(centre,min(min(a,b),min(c,d)));
    let high=max(centre,max(max(a,b),max(c,d)));
    let average=(a+b+c+d)*0.25;
    let contrast=temporal_luminance(high-low)/max(temporal_luminance(high),0.04);
    let amount=0.3*(1.0-smoothstep(0.35,0.9,contrast));
    let limit=(high-low)*0.1;
    return clamp(centre+clamp((centre-average)*amount,-limit,limit),low,high);
}

// Preserve the dark linear response and highlight hue until display-gamut compression is needed.
fn temporal_display_curve(rgb:vec3<f32>)->vec3<f32> {
    let value=max(rgb,vec3(0.0));
    let luma=temporal_luminance(value);
    if(luma<=0.000001){return value;}
    let shoulder=max(luma-0.6,0.0);
    let mapped=min(luma,0.6)+0.4*shoulder/(shoulder+0.4);
    let colour=value*(mapped/luma);
    let peak=max(max(colour.r,colour.g),colour.b);
    let compression=clamp((peak-1.0)/max(peak-mapped,0.000001),0.0,1.0);
    return clamp(mix(colour,vec3(mapped),compression),vec3(0.0),vec3(1.0));
}
