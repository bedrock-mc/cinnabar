// Material-owned overrides split deduplicated carrier references without changing fallback UVs.
@if(ENHANCED || ENHANCED_SHADOW)
fn authored_material_ref(material_id:u32,texture_ref:u32)->u32 {
    let carrier_ref=texture_ref & ((GPU_TEXTURE_PAGE_MASK << GPU_TEXTURE_PAGE_SHIFT) | TEXTURE_LAYER_MASK);
    let words=arrayLength(&enhanced_texture_refs);
    var low=0u;
    var high=0u;
    if(words>AUTHORED_REFERENCE_BASE_WORDS){
        high=(words-AUTHORED_REFERENCE_BASE_WORDS)/AUTHORED_REFERENCE_OVERRIDE_WORDS;
    }
    while(low<high){
        let middle=low+(high-low)/2u;
        let start=AUTHORED_REFERENCE_BASE_WORDS+middle*AUTHORED_REFERENCE_OVERRIDE_WORDS;
        let material=enhanced_texture_refs[start];
        let texture=enhanced_texture_refs[start+1u];
        if(material==material_id && texture==carrier_ref){return enhanced_texture_refs[start+2u];}
        if(material<material_id || (material==material_id && texture<carrier_ref)){low=middle+1u;}
        else{high=middle;}
    }
    return enhanced_texture_refs[authored_texture_slot(texture_ref)];
}
