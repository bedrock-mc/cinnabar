"""Generate the extension's Rust declarations and Go canonical document codecs."""
import argparse
import json
from pathlib import Path
import re
import subprocess
from constraints import render as render_constraints

ROOT = Path(__file__).resolve().parents[3]
SCHEMA = Path(__file__).with_name('extension.json')
HEADER = '// Generated from crates/server-experience/schema/extension.json; do not edit.\n'


def go_type(typ):
    """Map one schema type to the existing server API's Go representation."""
    if typ.startswith('Option<'):
        return '*' + go_type(typ[7:-1])
    if typ.startswith('BTreeSet<'):
        inner = typ[9:-1].split('::')[-1]
        return 'Permissions' if inner == 'Permission' else '[]' + go_type(inner)
    if 'BTreeMap<' in typ:
        return 'map[string]Permissions'
    if typ.startswith('Vec<'):
        return '[]' + go_type(typ[4:-1])
    if typ.startswith('Box<'):
        return go_type(typ[4:-1])
    return {'String': 'string', 'u8': 'uint8', 'u16': 'uint16', 'u32': 'uint32', 'u64': 'uint64', 'i64': 'int64', 'Scalar': 'Scalar'}.get(typ, typ.split('::')[-1])


def append_value(typ, value):
    """Emit an append into a canonical object for one schema value."""
    if typ == 'String': return f'o.str({value})'
    if typ.startswith('u'): return f'o.uint(uint64({value}))'
    if typ == 'i64': return f'o.int({value})'
    if typ == 'bool': return f'o.key(); o.b = strconv.AppendBool(o.b, {value})'
    if typ == 'Vec<String>': return f'o.key(); o.add(appendStrings(o.b, {value}))'
    if typ == 'Vec<Scalar>': return f'o.value(Record({value}))'
    if typ == 'Vec<Field>': return f'o.value(fieldList({value}))'
    if typ.startswith('Vec<'): return f'o.value(list[{go_type(typ[4:-1])}]({value}))'
    if typ == 'BTreeSet<String>': return f'o.key(); o.add(appendSet(o.b, {value}))'
    if typ == 'BTreeSet<u16>': return f'o.key(); o.add(appendVersions(o.b, {value}))'
    if 'BTreeMap<' in typ: return f'o.key(); o.add(appendPermissionMap(o.b, {value}))'
    if typ == 'Box<Field>': return f'o.key(); o.add(appendField(o.b, {value}))'
    return f'o.value({value})'


def read_value(typ, value, reader='r'):
    """Emit a typed read without relaxing the native strict JSON reader."""
    if typ == 'String': return f'{value}, err = {reader}.str()'
    if typ.startswith('u'): return f'{value}, err = readUint[{go_type(typ)}]({reader})'
    if typ == 'i64': return f'{value}, err = {reader}.int64()'
    if typ == 'bool': return f'{value}, err = {reader}.boolean()'
    if typ == 'Vec<String>': return f'{value}, err = readStrings({reader})'
    if typ == 'Vec<Scalar>': return f'err = (*Record)(&{value}).decodeJSON({reader})'
    if typ == 'Vec<Field>': return f'{value}, err = readFields({reader})'
    if typ.startswith('Vec<'): return f'{value}, err = readList[{go_type(typ[4:-1])}]({reader})'
    if typ == 'BTreeSet<String>': return f'{value}, err = readSet({reader})'
    if typ == 'BTreeSet<u16>': return f'{value}, err = readVersions({reader})'
    if 'BTreeMap<' in typ: return f'{value}, err = readPermissionMap({reader})'
    if typ == 'Box<Field>': return f'{value}, err = readField({reader})'
    return f'err = {value}.decodeJSON({reader})'


def go_record(record):
    """Emit one declaration and both codecs in schema field order."""
    name, fields = record['name'], record['fields']
    member = name[0].lower() + name[1:] + 'Fields'
    out = [f'// {name} is the schema-defined {name} document.', f'type {name} struct {{']
    if name == 'Fragment':
        out += ['Header Envelope', 'Part Part']
    else:
        out += [f"{f['go']} {go_type(f['type'])}" for f in fields]
    out += ['}', f'var {member} = []string{{' + ','.join(json.dumps(f['name']) for f in fields) + '}']
    out += [f'// appendJSON writes {name} in canonical field order.', f'func (v {name}) appendJSON(b []byte) ([]byte, error) {{', f'o := object{{b:b, names:{member}}}']
    for f in fields:
        value = 'v.' + f['go']; typ = f['type']
        if name == 'Fragment': value = 'v.Part' if f['name'] == 'fragment' else 'v.Header.' + f['go']
        if typ.startswith('Option<'):
            out += [f'if {value} != nil {{', append_value(typ[7:-1], '*'+value), '} else {', 'o.skip()' if f['skip'] else 'o.key(); o.b = append(o.b, "null"...)', '}']
        elif f['skip']:
            out += [f'if len({value}) > 0 {{', append_value(typ,value), '} else { o.skip() }']
        else: out += [append_value(typ,value)]
    out += ['return o.end()', '}', f'// decodeJSON reads a strict {name} object.', f'func (v *{name}) decodeJSON(r *reader) error {{', f'*v = {name}{{}}', f'return r.fields({member}, func(name string) (err error) {{', 'switch name {']
    for f in fields:
        value='v.'+f['go']; typ=f['type']
        if name == 'Fragment': value = 'v.Part' if f['name'] == 'fragment' else 'v.Header.'+f['go']
        out += [f"case {json.dumps(f['name'])}:"]
        if typ.startswith('Option<'):
            inner=typ[7:-1]
            out += ['if !r.null() {', f'var item {go_type(inner)}', read_value(inner,'item'), f'{value} = &item', '}']
        else: out += [read_value(typ,value)]
    opts=''.join(', '+json.dumps(f['name']) for f in fields if f['optional'])
    out += ['}', 'return err', '}' + opts + ')', '}']
    return '\n'.join(out)+'\n'


def rust_record(record):
    """Emit serde declarations while preserving their derives and optional-field rules."""
    out=[f"#[derive({record['derive']})]", '#[serde(deny_unknown_fields)]', f"pub struct {record['name']} {{"]
    for f in record['fields']:
        if f['skip']:
            predicate='Option::is_none' if f['type'].startswith('Option<') else 'BTreeSet::is_empty'
            out += [f'#[serde(default, skip_serializing_if = "{predicate}")]']
        out += [f"pub {f['name']}: {f['type']},"]
    return '\n'.join(out+['}'])+'\n'


def rust_enum(enum):
    """Emit plain, internally tagged and adjacently tagged enum declarations."""
    out=[f"#[derive({enum['derive']})]"]
    attrs=['rename_all = "snake_case"']
    if enum.get('tag'): attrs += [f"tag = \"{enum['tag']}\"", 'deny_unknown_fields']
    if enum.get('content'): attrs += [f"content = \"{enum['content']}\""]
    out += ['#[serde('+', '.join(attrs)+')]', f"pub enum {enum['name']} {{"]
    for v in enum['variants']:
        if 'type' in v: out += [f"{v['rust']}({v['type']}),"]
        elif v.get('fields'): out += [v['rust']+' {']+[f"{f['name']}: {f['type']}," for f in v['fields']]+['},']
        elif enum.get('tag') and not enum.get('content'): out += [v['rust']+' {},']
        else: out += [v['rust']+',']
    return '\n'.join(out+['}'])+'\n'


def go_enum(enum):
    """Emit an ordered string enum and its strict native Go codecs."""
    name=enum['name']; names=name.lower()+'Names'; out=[f'type {name} uint8', 'const (']
    out += [v['go']+(f' {name} = iota' if i==0 else '') for i,v in enumerate(enum['variants'])]
    out += [')', f'var {names} = [...]string{{'+','.join(json.dumps(v['json']) for v in enum['variants'])+'}']
    out += [f'// String returns the schema name, or a diagnostic for an invalid enum.', f'func (v {name}) String() string {{ if int(v)<len({names}) {{ return {names}[v] }}; return fmt.Sprintf("{name}(%d)", uint8(v)) }}', f'// appendJSON encodes one declared enum name.', f'func (v {name}) appendJSON(b []byte) ([]byte,error) {{ if int(v)>=len({names}) {{ return nil,fmt.Errorf("invalid {name} %d",v) }}; return appendString(b,{names}[v]) }}', f'// decodeJSON rejects names outside the schema.', f'func (v *{name}) decodeJSON(r *reader) error {{ name,err:=r.str(); if err!=nil {{ return err }}; i:=slices.Index({names}[:],name); if i<0 {{ return r.errorf("unknown {name} %q",name) }}; *v={name}(i); return nil }}']
    return '\n'.join(out)+'\n'


def go_control(enum):
    """Emit the adjacent-tag union without changing its pointer-based Go API."""
    out=['type Control struct {']
    for v in enum['variants']: out += [v['go']+' '+('*'+v.get('type',v['go']) if v['json']!='disabled' else 'bool')]
    out += ['}', 'var controlFields=[]string{"kind","body"}', '// appendJSON encodes exactly one control variant.', 'func (c Control) appendJSON(b []byte)([]byte,error){ var kind string; var body CanonicalValue; count:=0']
    for v in enum['variants']:
        cond=f"c.{v['go']}" if v['json']=='disabled' else f"c.{v['go']}!=nil"
        value='nil' if v['json']=='disabled' else f"*c.{v['go']}"
        out += [f'if {cond} {{ kind,body={json.dumps(v["json"])},{value}; count++ }}']
    out += ['if count!=1 { return nil,fmt.Errorf("a control message holds %d variants, not 1",count) }', 'b=append(b,`{"kind":"`...); b=append(b,kind...); b=append(b,\'"\'); if body!=nil { b=append(b,`,"body":`...); var err error; b,err=body.appendJSON(b); if err!=nil{return nil,err} }; return append(b,\'}\'),nil', '}', '// decodeJSON reads the tagged body and rejects unknown or duplicate members.', 'func (c *Control) decodeJSON(r *reader) error { *c=Control{}; kind,raw,err:=r.tagged("kind"); if err!=nil{return err}; var body Target; switch kind {']
    for v in enum['variants']:
        out += [f'case {json.dumps(v["json"])}:']
        out += [f"c.{v['go']}=true" if v['json']=='disabled' else f"c.{v['go']}=new({v.get('type',v['go'])}); body=c.{v['go']}"]
    out += ['default: return r.errorf("unknown control kind %q",kind)', '}', 'return within(raw,func(sub *reader)error{ member:=func(name string)error{ if name=="kind"{_,err:=sub.str();return err}; if body!=nil{return body.decodeJSON(sub)}; if sub.null(){return nil}; return sub.errorf("a disabled message has no body") }; if body==nil{return sub.fields(controlFields,member,"body")}; return sub.fields(controlFields,member) })', '}']
    return '\n'.join(out)+'\n'


def go_fields(enum):
    """Emit the recursive internal-tag field union using schema-defined members."""
    out=['type Field interface { field() }', 'var fieldMembers=map[string][]string{']
    for v in enum['variants']: out += [json.dumps(v['json'])+':{'+','.join(json.dumps(x) for x in ['type']+[f['name'] for f in v['fields']])+'},']
    out += ['}']
    for v in enum['variants']:
        out += [f"type {v['go']} struct {{"]+[f"{f['go']} {go_type(f['type'])}" for f in v['fields']]+['}', '// field identifies a declared field variant.', f"func ({v['go']}) field() {{}}"]
    out += ['// appendField writes an internally tagged field declaration.', 'func appendField(b []byte, field Field)([]byte,error){switch f:=field.(type){']
    for v in enum['variants']:
        out += [f"case {v['go']}:", f'o:=object{{b:b,names:fieldMembers[{json.dumps(v["json"])}]}}; o.str({json.dumps(v["json"])})']
        if not v['fields']: out += ['_ = f']
        out += [append_value(f['type'],'f.'+f['go']) for f in v['fields']]+['return o.end()']
    out += ['};return nil,errors.New("a field with no type")','}', '// readField reads only the members of the declared variant.', 'func readField(r *reader)(Field,error){ kind,raw,err:=r.tagged("type");if err!=nil{return nil,err}; switch kind {']
    for v in enum['variants']:
        out += [f'case {json.dumps(v["json"])}:', f"var v {v['go']}", f'err=within(raw,func(sub *reader)error{{sub.depth=r.depth;return sub.fields(fieldMembers[kind],func(name string)(err error){{switch name{{case "type":_,err=sub.str()']
        for f in v['fields']: out += [f'case {json.dumps(f["name"])}:',read_value(f['type'],'v.'+f['go'],'sub')]
        out += ['};return err})});if err!=nil{return nil,err};return v,nil']
    out += ['};return nil,r.errorf("unknown field type %q",kind)','}']
    return '\n'.join(out)+'\n'


def go_scalars(enum):
    """Emit the adjacent-tag scalar codecs against the independent runtime value type."""
    out=['var scalarFields=[]string{"type","value"}', '// appendScalar requires exactly one value and writes its schema tag.', 'func appendScalar(b []byte,s Scalar)([]byte,error){count:=0']
    for v in enum['variants']: out += [f"if s.{v['go']}!=nil{{count++}}"]
    out += ['if count!=1{return nil,fmt.Errorf("a scalar holds %d values, not 1",count)}; o:=object{b:b,names:scalarFields}; switch {']
    for v in enum['variants']:
        out += [f"case s.{v['go']}!=nil:",f'o.str({json.dumps(v["json"])})',append_value(v['type'],'*s.'+v['go'])]
    out += ['};return o.end()', '}', '// readScalar rejects undeclared tags and preserves the recursion budget.', 'func readScalar(r *reader)(Scalar,error){kind,raw,err:=r.tagged("type");if err!=nil{return Scalar{},err};var s Scalar;err=within(raw,func(sub *reader)error{sub.depth=r.depth;return sub.fields(scalarFields,func(name string)(err error){if name=="type"{_,err=sub.str();return err};switch kind{']
    for v in enum['variants']:
        out += [f'case {json.dumps(v["json"])}:',f"var value {go_type(v['type'])}",read_value(v['type'],'value','sub')]
        if v['type']=='Vec<Scalar>': out += ['if value==nil{value=[]Scalar{}}']
        out += [f"s.{v['go']}=&value"]
    out += ['default:return sub.errorf("unknown scalar type %q",kind)', '};return err})});if err!=nil{return Scalar{},err};return s,nil','}']
    return '\n'.join(out)+'\n'


def outputs(schema):
    """Render every binding from the single ordered schema."""
    rust={}; go={}
    record_names={record['name'] for record in schema['records']}
    for r in schema['records']:
        if r['module']: rust.setdefault(r['module'],[]).append(rust_record(r))
        module=r['module'] or 'session'
        go.setdefault(module,[]).append(go_record(r))
    for e in schema['enums']:
        rust.setdefault(e['module'],[]).append(rust_enum(e))
        if e['name']=='Control':
            for v in e['variants']:
                if 'fields' in v and v['go'] not in record_names:
                    go.setdefault(e['module'],[]).append(go_record({'name':v['go'],'fields':v['fields']}))
        render={'Field':go_fields,'Scalar':go_scalars,'Control':go_control}.get(e['name'],go_enum)
        go.setdefault(e['module'],[]).append(render(e))
    rules_rust,rules_go=render_constraints(schema)
    for module,text in rules_rust.items(): rust.setdefault(module,[]).append(text)
    for module,text in rules_go.items(): go.setdefault(module,[]).append(text)
    constants=[]
    for c in schema['constants']:
        if c['module']: rust.setdefault(c['module'],[]).append(f"pub const {c['name']}: {c['type']} = {c['value']};\n")
        if c.get('go'):
            value=c.get('value')
            if c.get('external'):
                source=(ROOT/c['external']['path']).read_text()
                value=re.search(r'pub const '+c['external']['name']+r': [^=]+ = ([^;]+);',source).group(1)
            if value.startswith('b"'): value=value[1:]
            value=value.replace('\\0','\\x00')
            constants.append(f"{c['go']} = {value}")
    result={Path('tools/localserver/cinnabarext/limits.go'):HEADER+'package cinnabarext\nconst(\n'+'\n'.join(constants)+f"\nmaxDepth={schema['json']['recursion_limit']}\n)\n"}
    for module,parts in rust.items():
        path=Path('crates/server-experience/src')/f'{module}.rs'
        generated=HEADER+'\n'.join(parts)
        source=(ROOT/path).read_text()
        begin='// BEGIN GENERATED EXTENSION CONTRACT\n'
        end='// END GENERATED EXTENSION CONTRACT\n'
        if begin in source:
            before,rest=source.split(begin,1)
            _,after=rest.split(end,1)
        else:
            before,after=source.split(f'include!("contract/{module}.rs");\n',1)
        result[path]=before+begin+generated+end+after
    for module,parts in go.items():
        imports={'manifest':['fmt','slices','strings'], 'negotiation':[], 'crypto':[], 'session':['fmt'], 'wire':['errors','fmt','slices','strconv']}[module]
        result[Path(f'tools/localserver/cinnabarext/contract_{module}_generated.go')]=HEADER+'package cinnabarext\n'+('import('+''.join(json.dumps(i)+'\n' for i in imports)+')\n' if imports else '')+'\n'.join(parts)
    return result


def main():
    """Write bindings, or compare formatter-normalized output in CI."""
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check',action='store_true')
    args=parser.parse_args()
    schema=json.loads(SCHEMA.read_text())
    if schema['json'] != {'unknown_fields':'deny','duplicate_struct_fields':'deny','missing_option':'null','set_order':'sorted_unique','map_order':'sorted_keys','unicode':'utf8','recursion_limit':128}:
        raise ValueError('unsupported canonical JSON rules')
    if schema['signing'] != {'algorithm':'ed25519','input':'domain_then_canonical_payload','hex':'lowercase'}:
        raise ValueError('unsupported signing rules')
    if schema['constraints']['wire_limits'] != 'ascending_nonzero':
        raise ValueError('unsupported wire limit ordering')
    for relative,text in outputs(schema).items():
        if relative.suffix=='.rs':
            command=['rustfmt','--edition','2024','--emit','stdout']
        else: command=['gofmt']
        data=subprocess.run(command,input=text,text=True,capture_output=True,check=True).stdout
        path=ROOT/relative
        if args.check:
            if not path.exists() or path.read_text()!=data: raise SystemExit(f'{relative} is stale; regenerate extension contracts')
        else:
            path.parent.mkdir(parents=True,exist_ok=True)
            path.write_text(data)


if __name__=='__main__': main()
