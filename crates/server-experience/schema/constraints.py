"""Render shared extension admission rules for each language's native values."""


def render(schema):
    """Return rules that use schema names and keep live execution in each owning process."""
    rules=schema['constraints']; identifier=rules['identifier']; fields=next(e for e in schema['enums'] if e['name']=='Field')
    rust=[]; go=[]
    for v in fields['variants']:
        kind=v['admission']; r=v['rust']; g=v['go']
        if kind=='bool': a=f'(Field::{r} {{}}, Scalar::{r}(_)) => true,'; b=f'case {g}: return value.Bool!=nil'
        elif kind=='range': a=f'(Field::{r} {{ min,max }}, Scalar::{r}(value)) => min<=value && value<=max,'; b=f'case {g}: return value.Integer!=nil && f.Min<=*value.Integer && *value.Integer<=f.Max'
        elif kind=='utf8_bytes': a=f'(Field::{r} {{ max_bytes }}, Scalar::{r}(value)) => value.len()<=usize::from(*max_bytes),'; b=f'case {g}: return value.Text!=nil && len(*value.Text)<=int(f.MaxBytes)'
        elif kind=='choice': a=f'(Field::{r} {{ variants }}, Scalar::{r}(value)) => value<variants,'; b=f'case {g}: return value.Choice!=nil && *value.Choice<f.Variants'
        elif kind=='list': a=f'(Field::{r} {{ item,max_items }}, Scalar::{r}(items)) => items.len()<=usize::from(*max_items) && items.iter().all(|value|item.admits(value)),'; b=f'case {g}: return value.List!=nil && len(*value.List)<=int(f.MaxItems) && !slices.ContainsFunc(*value.List,func(value Scalar)bool{{return !admits(f.Item,value)}})'
        elif kind=='record': a=f'(Field::{r} {{ fields }}, Scalar::{r}(values)) => fields.len()==values.len() && fields.iter().zip(values).all(|(field,value)|field.admits(value)),'; b=f'case {g}: if value.Record==nil || len(*value.Record)!=len(f.Fields){{return false}};for i,field:=range f.Fields{{if !admits(field,(*value.Record)[i]){{return false}}}};return true'
        else: raise ValueError(f'unknown admission {kind}')
        rust.append(a);go.append(b)
    depth=rules['channels']['depth']; count=rules['channels']['fields']
    rust_field=f'''impl Field {{
    /// Checks container nesting and record field counts before accepting a declaration.
    fn declared(&self,depth:usize)->bool {{ match self {{
    Field::List {{ item,.. }} => depth>0 && item.declared(depth-1),
    Field::Record {{ fields }} => depth>0 && fields.len()<={count} && fields.iter().all(|field|field.declared(depth-1)),
    _=>true }} }}
    /// Checks the value's type and every schema-defined bound recursively.
    fn admits(&self,value:&Scalar)->bool {{ match (self,value) {{ {''.join(rust)} _=>false }} }}
    }}
    '''
    go_field=f'''// declared checks container nesting and record field counts.
    func declared(f Field,depth int)bool{{switch f:=f.(type){{case ListField:return depth>0 && declared(f.Item,depth-1);case RecordField:return depth>0 && len(f.Fields)<=MaxChannelFields && !slices.ContainsFunc(f.Fields,func(field Field)bool{{return !declared(field,depth-1)}});case BoolField,IntegerField,TextField,ChoiceField:return true}};return false}}
    // admits checks the value's type and every schema-defined bound recursively.
    func admits(f Field,value Scalar)bool{{switch f:=f.(type){{{chr(10).join(go)}}};return false}}
    '''
    chars=identifier['extra_ascii']; max_bytes=identifier['max_bytes']
    rust_id=f'''/// Restricts identifiers to the shared portable namespace.
    pub fn identifier(text:&str)->bool {{ !text.is_empty() && text.len()<={max_bytes} && text.bytes().all(|b|b.is_ascii_lowercase() || b.is_ascii_digit() || b"{chars}".contains(&b)) }}
    /// Lists permissions with application adapters in the developer preview.
    pub fn implemented_permissions()->BTreeSet<Permission>{{ BTreeSet::from([{','.join('Permission::'+v['rust'] for v in schema['enums'][0]['variants'] if v.get('implemented'))}]) }}
    '''
    go_id=f'''// Identifier restricts names to the shared portable namespace.
    func Identifier(text string)bool{{if text=="" || len(text)>MaxIdentifierBytes{{return false}};for i:=range len(text){{c:=text[i];if !(c>='a'&&c<='z'||c>='0'&&c<='9'||strings.IndexByte("{chars}",c)>=0){{return false}}}};return true}}
    const ImplementedPermissions=Permissions({ '|'.join('1<<'+v['go'] for v in schema['enums'][0]['variants'] if v.get('implemented')) })
    '''
    limit_fields=next(r for r in schema['records'] if r['name']=='Limits')['fields']
    rust_limits='''impl Limits {
    /// Checks that the selected ceilings are nonzero and ordered.
    fn ordered(&self)->bool { '''+' && '.join(['0<self.'+limit_fields[0]['name']]+['self.'+a['name']+'<=self.'+b['name'] for a,b in zip(limit_fields,limit_fields[1:])])+''' }
    }
    '''
    go_limits='''// ordered checks that the selected ceilings are nonzero and ordered.
    func(l Limits)ordered()bool{return '''+' && '.join(['0<l.'+limit_fields[0]['go']]+['l.'+a['go']+'<=l.'+b['go'] for a,b in zip(limit_fields,limit_fields[1:])])+'''}
    // lowest lowers each selected ceiling to the corresponding offer ceiling.
    func(l Limits)lowest(other Limits)Limits{return Limits{'''+','.join('min(l.'+f['go']+',other.'+f['go']+')' for f in limit_fields)+'''}}
    '''
    rust_sign='''/// Builds the domain-separated bytes consumed by native Ed25519 signing.
    fn signing_bytes(domain:&[u8],payload:&[u8])->Vec<u8>{let mut bytes=Vec::with_capacity(domain.len()+payload.len());bytes.extend_from_slice(domain);bytes.extend_from_slice(payload);bytes}
    '''
    go_sign='''// signingBytes builds the domain-separated bytes consumed by native Ed25519 signing.
    func signingBytes(domain string,payload []byte)[]byte{return append([]byte(domain),payload...)}
    '''
    rust_field += f"""impl Channel {{
    /// Checks identifiers, field counts and nesting against the shared ceilings.
    pub fn declared(&self)->bool{{ identifier(&self.id) && self.fields.len()<={count} && self.fields.iter().all(|field|field.declared({depth})) }}
    /// Checks the record's direction, positional fields and encoded byte ceiling.
    pub fn validate(&self,payload:&[Scalar],direction:Direction,max_bytes:usize)->Result<()>{{
    ensure!(self.declared() && self.direction==direction,"channel denied");
    ensure!(payload.len()==self.fields.len(),"record field count mismatch");
    for(field,value)in self.fields.iter().zip(payload){{ensure!(field.admits(value),"record field rejected");}}
    ensure!(serde_json::to_vec(payload)?.len()<=max_bytes,"payload too large");Ok(()) }}
    }}
    """
    go_field += """// Declared checks identifiers, field counts and nesting against the shared ceilings.
    func(c Channel)Declared()bool{return Identifier(c.ID) && len(c.Fields)<=MaxChannelFields && !slices.ContainsFunc(c.Fields,func(f Field)bool{return !declared(f,MaxFieldDepth)})}
    // Validate checks direction, positional fields and the encoded byte ceiling.
    func(c Channel)Validate(payload []Scalar,direction Direction,maxBytes int)error{
    if !c.Declared() || c.Direction!=direction{return errors.New("channel denied")};if len(payload)!=len(c.Fields){return errors.New("record field count mismatch")};
    for i,field:=range c.Fields{if !admits(field,payload[i]){return fmt.Errorf("record field %d rejected",i)}}
    encoded,err:=Encode(Record(payload));if err!=nil{return err};if len(encoded)>maxBytes{return errors.New("payload too large")};return nil}
    """
    return {'wire':rust_field,'manifest':rust_id,'negotiation':rust_limits,'crypto':rust_sign}, {'wire':go_field,'manifest':go_id,'negotiation':go_limits,'crypto':go_sign}
