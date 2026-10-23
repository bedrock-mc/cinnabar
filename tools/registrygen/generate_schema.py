#!/usr/bin/env python3
"""Generate the Go and Rust block-registry contract from its shared schema."""

import argparse
import json
from pathlib import Path
import subprocess


ROOT = Path(__file__).resolve().parents[2]
SCHEMA = ROOT / "crates/assets/data/registry-schema.json"
SERDE_DERIVE = ", serde::Serialize, serde::Deserialize"  # Opt-in for types stored in the compiled-pack disk cache.


def validate(schema):
    """Reject ambiguous discriminants, flag bits and unsupported wire fields."""
    if len(schema["magic"].encode("ascii")) != 8:
        raise ValueError("registry magic must contain eight ASCII bytes")
    for key in ("physics_magic", "physics_legacy_magic"):
        if len(schema[key].encode("ascii")) != 8:
            raise ValueError("physics magic must contain eight ASCII bytes")
    for key in ("physics_header_bytes", "physics_record_header_bytes"):
        if type(schema[key]) is not int or schema[key] <= 0:
            raise ValueError("physics header sizes must be positive integers")
    offset = schema["physics_movement_semantics_offset"]
    if type(offset) is not int or not 0 <= offset < schema["physics_record_header_bytes"]:
        raise ValueError("movement semantics must lie within the physics record header")
    for enum in schema["enums"]:
        values = enum["values"]
        numbers = [v["value"] for v in values]
        names = [v["name"] for v in values]
        if len(set(numbers)) != len(numbers) or len(set(names)) != len(names):
            raise ValueError(f"duplicate enum member in {enum['name']}")
        if any(not 0 <= v <= 255 for v in numbers):
            raise ValueError("registry enums must fit in a byte")
        if numbers != list(range(numbers[0], numbers[-1] + 1)):
            raise ValueError("registry enums need ascending contiguous discriminants")
        if enum["default"] and enum["default"] not in names:
            raise ValueError("enum default must name a member")
    for flags in schema["flags"]:
        bits = [v["bit"] for v in flags["values"]]
        if len(set(bits)) != len(bits) or any(not 0 <= v < (64 if flags.get("type") == "u64" else 8) for v in bits):
            raise ValueError("registry flags need unique byte-sized bits")
    fields = schema["record_header"]
    if len({v["name"] for v in fields}) != len(fields):
        raise ValueError("duplicate record header field")
    if any(v["type"] not in {"u8", "u16", "u32", "u64", "u32[8]"} for v in fields):
        raise ValueError("unsupported record header field")


def rust_properties(schema):
    """Render property keys and their pinned value domains without a second handwritten list."""
    document = json.loads((SCHEMA.parent / schema["block_properties"]).read_text())
    properties = document["properties"]
    names = ["".join(part.title() for part in p["name"].replace(":", "_").split("_")) for p in properties]
    if len(set(names)) != len(names):
        raise ValueError("property variants collide")
    lines = ["", "/// A state key from the pinned block property vocabulary.", "#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]", "#[repr(u16)]", "pub enum BlockProperty {"]
    lines += [f"    {name} = {i}," for i, name in enumerate(names)]
    lines += ["}", "impl BlockProperty {", "    /// Lists every key in vocabulary order.", "    pub const ALL: &[Self] = &["]
    lines += [f"        Self::{name}," for name in names]
    lines += ["    ];", "    /// Returns the serialized state key.", "    pub const fn name(self) -> &'static str {", "        match self {"]
    lines += [f'            Self::{name} => {json.dumps(p["name"])},' for name, p in zip(names, properties)]
    lines += ["        }", "    }", "    /// Resolves a key once while admitting a registry record.", "    pub fn from_name(name: &str) -> Option<Self> {", "        match name {"]
    lines += [f'            {json.dumps(p["name"])} => Some(Self::{name}),' for name, p in zip(names, properties)]
    lines += ["            _ => None,", "        }", "    }", "    /// Returns the complete pinned domain across all blocks; one block may admit fewer values.", "    pub const fn domain(self) -> super::PropertyDomain {", "        match self {"]
    for name, p in zip(names, properties):
        values = [v["value"] for v in p["values"]]
        if p["type"] == "bool":
            domain = "Bool"
        elif p["type"] == "int":
            domain = "Int(&[" + ",".join(str(v) for v in values) + "])"
        else:
            domain = "String(&[" + ",".join(json.dumps(v) for v in values) + "])"
        lines.append(f"            Self::{name} => super::PropertyDomain::{domain},")
    lines += ["        }", "    }", "}"]
    narrowed = {}
    for name, p in zip(names, properties):
        for group in p.get("block_domains", []):
            if p["type"] == "bool":
                raise ValueError(f"boolean property narrows per block: {p['name']}")
            values = ",".join(json.dumps(v) for v in group["values"])
            kind = "Int" if p["type"] == "int" else "String"
            for block in group["blocks"]:
                narrowed.setdefault(block, []).append(f"(BlockProperty::{name}, super::PropertyDomain::{kind}(&[{values}]))")
    arms = {}
    for block, entries in sorted(narrowed.items()):
        arms.setdefault(tuple(entries), []).append(block)
    lines += ["/// Returns the narrower domains one block's states admit, in key order.", "pub fn block_domains(block: &str) -> &'static [(BlockProperty, super::PropertyDomain)] {", "    match block {"]
    for entries, blocks in sorted(arms.items(), key=lambda arm: arm[1]):
        lines.append(f"        {' | '.join(json.dumps(b) for b in blocks)} => &[{', '.join(entries)}],")
    lines += ["        _ => &[],", "    }", "}"]
    return lines


def rust_declarations(schema):
    """Render the reader's public types and bounded wire-header decoder."""
    lines = [
        "// Generated by tools/registrygen/generate_schema.py; edit registry-schema.json.",
        "use bitflags::bitflags;",
        "use super::Reader;",
        "use crate::facts::{BlockProperty, StateValue};",
        "use crate::AssetError;",
        "",
        f'pub(super) const REGISTRY_MAGIC: &[u8; 8] = b"{schema["magic"]}";',
        f'pub(super) const PREVIOUS_REGISTRY_MAGIC: &[u8; 8] = b"{schema["previous_magic"]}";',
        f'pub const PHYSICS_REGISTRY_MAGIC: &[u8; 8] = b"{schema["physics_magic"]}";',
        f'pub const LEGACY_PHYSICS_REGISTRY_MAGIC: &[u8; 8] = b"{schema["physics_legacy_magic"]}";',
        f'pub const PHYSICS_REGISTRY_HEADER_BYTES: usize = {schema["physics_header_bytes"]};',
        f'pub const PHYSICS_RECORD_HEADER_BYTES: usize = {schema["physics_record_header_bytes"]};',
        f'pub const PHYSICS_MOVEMENT_SEMANTICS_OFFSET: usize = {schema["physics_movement_semantics_offset"]};',
        f'pub(super) const LEGACY_REGISTRY_PROTOCOL: u32 = {schema["legacy_protocol"]};',
    ]
    size = sum({"u8": 1, "u16": 2, "u32": 4, "u64": 8, "u32[8]": 32}[f["type"]]
               for f in schema["record_header"])
    lines.append(f"pub(super) const RECORD_HEADER_BYTES: usize = {size};")
    previous_size = sum({"u8": 1, "u16": 2, "u32": 4, "u64": 8, "u32[8]": 32}[f["type"]]
                        for f in schema["record_header"] if "previous_default" not in f)
    lines.append(f"pub(super) const PREVIOUS_RECORD_HEADER_BYTES: usize = {previous_size};")
    for limit in schema["limits"]:
        if limit["rust_name"]:
            lines.append(f'pub(super) const {limit["rust_name"]}: usize = {limit["value"]};')
    for flags in schema["flags"]:
        derive = "Debug, Clone, Copy, Default, PartialEq, Eq, Hash" + SERDE_DERIVE * flags.get("serde", False)
        lines += ["", "bitflags! {", f'    /// {flags["doc"]}',
                  f"    #[derive({derive})]",
                  f'    pub struct {flags["name"]}: {flags.get("type", "u8")} {{']
        for value in flags["values"]:
            if value.get("doc"):
                lines.append(f'        /// {value["doc"]}')
            lines.append(f'        const {value["name"]} = 1 << {value["bit"]};')
        lines += ["    }", "}"]

    for enum in schema["enums"]:
        derive = "Debug, Clone, Copy, PartialEq, Eq, Hash"
        if enum["default"]:
            derive += ", Default"
        derive += SERDE_DERIVE * enum.get("serde", False)
        name = enum["name"]
        lines += ["", "/// " + enum.get("doc", "A byte-sized registry discriminator."),
                  f"#[derive({derive})]", "#[repr(u8)]", f"pub enum {name} {{"]
        for value in enum["values"]:
            if value["name"] == enum["default"]:
                lines.append("    #[default]")
            lines.append(f'    {value["name"]} = {value["value"]},')
        lines.append("}")
        lines += ["", f"impl {name} {{", "    /// Resolves a compiler rule's declared enum name.", "    pub fn from_name(name: &str) -> Option<Self> {", "        match name {"]
        for value in enum["values"]:
            lines.append(f'            "{value["name"]}" => Some(Self::{value["name"]}),')
        lines += ["            _ => None,", "        }", "    }", "}"]
        if enum["read"]:
            lines += ["", f"impl {name} {{",
                      "    /// Decode a known discriminator and reject reserved values.",
                      f'    {enum["read_visibility"]} fn read(raw: u8) -> Result<Self, AssetError> {{',
                      "        match raw {"]
            for value in enum["values"]:
                lines.append(f'            {value["value"]} => Ok(Self::{value["name"]}),')
            lines += ["            _ => Err(AssetError::InvalidRegistryFlags(raw)),",
                      "        }", "    }", "}"]
    for struct in schema["structs"]:
        lines += ["", f'/// {struct["doc"]}',
                  f'#[derive({struct["derive"]})]', f'pub struct {struct["rust_name"]} {{']
        for field in struct["fields"]:
            visibility = "pub" if field["public"] else "pub(super)"
            lines.append(f'    {visibility} {field["rust_name"]}: {field["rust_type"]},')
        lines.append("}")
    for name in ("RegistryMetadata", "CollisionBox"):
        struct = next(s for s in schema["structs"] if s["rust_name"] == name)
        function = "read_registry_metadata" if name == "RegistryMetadata" else "read_collision_box"
        lines += ["", f"/// Read {name} fields in their declared little-endian order.",
                  f"pub(super) fn {function}(reader: &mut Reader<'_>) -> Result<{name}, AssetError> {{",
                  f"    Ok({name} {{"]
        for field in struct["fields"]:
            kind, field_name = field["rust_type"], field["rust_name"]
            lines.append(f'        {field_name}: reader.read_{kind}("{field_name}")?,')
        lines += ["    })", "}"]
    lines += ["", "/// The fixed-size portion of one little-endian record.",
              "pub(super) struct RecordHeader {"]
    for field in schema["record_header"]:
        kind = "[u32; 8]" if field["type"] == "u32[8]" else field["type"]
        lines.append(f'    pub(super) {field["name"]}: {kind},')
    lines += ["}", "", "/// Read the fixed header in schema order before validating its facts.",
              "pub(super) fn read_record_header(reader: &mut Reader<'_>, previous: bool) -> Result<RecordHeader, AssetError> {",
              "    Ok(RecordHeader {"]
    for field in schema["record_header"]:
        name, kind = field["name"], field["type"]
        if kind == "u32[8]":
            lines += [f"        {name}: {{", "            let mut values = [0; 8];",
                      "            for value in &mut values {",
                      f'                *value = reader.read_u32("record {name}")?;',
                      "            }", "            values", "        },"]
        else:
            expression = f'reader.read_{kind}("record {name}")?'
            if "previous_default" in field:
                expression = f'if previous {{ {field["previous_default"]} }} else {{ {expression} }}'
            lines.append(f'        {name}: {expression},')
    lines += ["    })", "}"]
    return "\n".join(lines) + "\n"


def go_declarations(schema):
    """Render generator declarations and the matching record-header encoder."""
    lines = ["// Code generated by generate_schema.py; DO NOT EDIT.", "package main", "",
             'import "encoding/binary"', "", "const (",
             f'    registryHeader = "{schema["magic"]}"',
             f'    previousRegistryHeader = "{schema["previous_magic"]}"',
             f'    physicsRegistryHeader = "{schema["physics_legacy_magic"]}"',
             f'    physicsRegistryActiveHeader = "{schema["physics_magic"]}"',
             f'    physicsRegistryHeaderBytes = {schema["physics_header_bytes"]}',
             f'    physicsRecordHeaderBytes = {schema["physics_record_header_bytes"]}',
             f'    physicsMovementSemanticsOffset = {schema["physics_movement_semantics_offset"]}',
             f'    registryProtocol = {schema["legacy_protocol"]}']
    for limit in schema["limits"]:
        lines.append(f'    {limit["go_name"]} = {limit["value"]}')
    lines.append(f'    registryHeaderBytes = {len(schema["magic"]) + 4 * len(next(s for s in schema["structs"] if s["go_name"] == "RegistryMetadata")["fields"])}')
    lines.append(f'    recordHeaderBytes = {sum({"u8": 1, "u16": 2, "u32": 4, "u64": 8, "u32[8]": 32}[f["type"]] for f in schema["record_header"])}')
    lines.append(f'    previousRecordHeaderBytes = {sum({"u8": 1, "u16": 2, "u32": 4, "u64": 8, "u32[8]": 32}[f["type"]] for f in schema["record_header"] if "previous_default" not in f)}')
    lines.append(f'    collisionBoxBytes = {4 * len(next(s for s in schema["structs"] if s["go_name"] == "CollisionBox")["fields"])}')
    lines.append(")")
    for flags in schema["flags"]:
        lines += ["", "const ("]
        source = [v for v in flags["values"] if v["source"]]
        for value in source:
            lines.append(f'    {value["go_name"]} uint{64 if flags.get("type") == "u64" else 8} = 1 << {value["bit"]}')
        lines.append(f'    {flags["go_all"]} = ' + " | ".join(v["go_name"] for v in source))
        lines.append(")")
    for enum in schema["enums"]:
        name = enum["name"]
        lines += ["", "// " + enum.get("doc", f"{name} is a byte-sized registry discriminator."),
                  f"type {name} uint8", "", "const ("]
        for value in enum["values"]:
            lines.append(f'    {enum["go_prefix"]}{value["name"]} {name} = {value["value"]}')
        lines += [")", f'const {enum["go_max"]} = {enum["go_prefix"]}{enum["values"][-1]["name"]}']
    family = next(e for e in schema["enums"] if e["name"] == "ModelFamily")
    lines += ["", "// modelFamilyByName resolves the shared compiler rule discriminator.", "var modelFamilyByName = map[string]ModelFamily{"]
    lines += [f'    "{v["name"]}": ModelFamily{v["name"]},' for v in family["values"]]
    lines.append("}")
    for struct in schema["structs"]:
        name = struct["go_name"]
        lines += ["", f"// {name} is a typed block-registry value.", f"type {name} struct {{"]
        for field in struct["fields"]:
            if field["go_name"] is None:
                continue
            lines.append(f'    {field["go_name"]} {field["go_type"]}')
        lines.append("}")
    for name in ("RegistryMetadata", "CollisionBox"):
        struct = next(s for s in schema["structs"] if s["go_name"] == name)
        function = "appendRegistryMetadata" if name == "RegistryMetadata" else "appendCollisionBox"
        lines += ["", f"// {function} writes {name} in its declared little-endian order.",
                  f"func {function}(encoded []byte, value {name}) []byte {{"]
        if name == "RegistryMetadata":
            lines.append("    if value.Protocol == registryProtocol { encoded = append(encoded, previousRegistryHeader...) } else { encoded = append(encoded, registryHeader...) }")
        for field in struct["fields"]:
            lines.append(f'    encoded = binary.LittleEndian.AppendUint32(encoded, uint32(value.{field["go_name"]}))')
        lines += ["    return encoded", "}"]
        function = "decode" + name
        lines += ["", f"// {function} reads fields from a previously length-checked slice.",
                  f"func {function}(data []byte) {name} {{", f"    return {name} {{"]
        for index, field in enumerate(struct["fields"]):
            lines.append(f'        {field["go_name"]}: {field["go_type"]}(binary.LittleEndian.Uint32(data[{index * 4}:{index * 4 + 4}])),')
        lines += ["    }", "}"]
    lines += ["", "// RecordHeader is the fixed-size portion of a little-endian record.",
              "type RecordHeader struct {"]
    for field in schema["record_header"]:
        kind = "[8]uint32" if field["type"] == "u32[8]" else "uint" + field["type"][1:]
        lines.append(f'    {go_field_name(field["name"])} {kind}')
    lines += ["}", "", "// decodeRecordHeader reads a previously length-checked record prefix.",
              "func decodeRecordHeader(data []byte, previous bool) RecordHeader {", "    var value RecordHeader"]
    offset = 0
    for field in schema["record_header"]:
        name, kind = go_field_name(field["name"]), field["type"]
        if kind == "u32[8]":
            lines += [f"    for index := range value.{name} {{",
                      f"        value.{name}[index] = binary.LittleEndian.Uint32(data[{offset}+index*4:{offset+4}+index*4])", "    }"]
            offset += 32
        elif kind == "u8":
            assignment = f"value.{name} = data[{offset}]"
            if "previous_default" in field:
                assignment = f"if !previous {{ {assignment} }}"
            lines.append(f"    {assignment}")
            offset += 1
        else:
            size = {"u16": 2, "u32": 4, "u64": 8}[kind]
            assignment = f"value.{name} = binary.LittleEndian.Uint{kind[1:]}(data[{offset}:{offset+size}])"
            lines.append(f"    if !previous {{ {assignment} }}" if "previous_default" in field else f"    {assignment}")
            offset += size
    lines += ["    return value", "}"]
    lines += ["", "// appendRecordHeader writes the already-validated fixed record header.",
              "func appendRecordHeader(encoded []byte, record Record, previous bool) []byte {"]
    for field in schema["record_header"]:
        kind, value = field["type"], field["go_value"]
        if kind == "u8":
            lines.append(f"    if !previous {{ encoded = append(encoded, {value}) }}" if "previous_default" in field else f"    encoded = append(encoded, {value})")
        elif kind == "u32[8]":
            lines += [f"    for _, value := range {value} {{",
                      "        encoded = binary.LittleEndian.AppendUint32(encoded, value)", "    }"]
        else:
            assignment = f"encoded = binary.LittleEndian.AppendUint{kind[1:]}(encoded, {value})"
            lines.append(f"    if !previous {{ {assignment} }}" if "previous_default" in field else f"    {assignment}")
    lines += ["    return encoded", "}"]
    return "\n".join(lines) + "\n"


def go_field_name(name):
    """Convert the schema's snake-case wire names to exported Go members."""
    return "".join(part.upper() if part == "id" else part.capitalize() for part in name.split("_"))


def rust_writer(schema):
    """Render production carrier field encoding in the asset compiler from the format schema."""
    lines=["// Generated by tools/registrygen/generate_schema.py; edit registry-schema.json.", "use assets::registry::{RegistryRecord, RegistryMetadata, CollisionBox, ModelStateField};"]
    for name in ["RegistryMetadata", "CollisionBox"]:
        struct=next(s for s in schema["structs"] if s["rust_name"]==name)
        function="append_metadata" if name=="RegistryMetadata" else "append_box"
        lines += ["/// Writes format fields in schema order without applying product policy.", f"pub(super) fn {function}(bytes:&mut Vec<u8>,value:&{name}){{"]
        for field in struct["fields"]:lines.append(f'bytes.extend_from_slice(&value.{field["rust_name"]}.to_le_bytes());')
        lines.append("}")
    lines += ["/// Writes the fixed header after the caller checks record bounds.","pub(super) fn append_header(bytes:&mut Vec<u8>,record:&RegistryRecord){"]
    for field in schema["record_header"]:
        value=field["rust_value"];kind=field["type"]
        if kind=="u8":lines.append(f"bytes.push({value});")
        elif kind=="u32[8]":
            enum=next(e for e in schema["enums"] if e["name"]=="ModelStateField")
            names=",".join("ModelStateField::"+v["name"] for v in enum["values"])
            lines.append(f"for field in [{names}]{{bytes.extend_from_slice(&record.model_state.get(field).unwrap_or(0).to_le_bytes());}}")
        else:lines.append(f"bytes.extend_from_slice(&({value}).to_le_bytes());")
    lines.append("}")
    for limit in schema["limits"]:
        if limit["rust_name"] in ["MAX_COLLISION_BOXES", "MAX_REGISTRY_RECORDS"]:
            lines.append(f'pub(super) const {limit["rust_name"]}:usize={limit["value"]};')
    lines += [f'pub(super) const REGISTRY_MAGIC:&[u8;8]=b"{schema["magic"]}";']
    return "\n".join(lines)+"\n"


def wire_fixture(schema):
    """Upgrade the independent previous wire fixture with empty compiled facts."""
    path = ROOT / "crates/assets/tests/fixtures/registry-wire.hex"
    data = bytes.fromhex(path.read_text())
    if data[:8] != schema["magic"].encode():
        if data[:8] != b"BREG1004":
            raise ValueError("unexpected wire fixture version")
        encoded = bytearray(schema["magic"].encode() + data[8:36])
        cursor = 36
        count = int.from_bytes(data[16:20], "little")
        for _ in range(count):
            header = bytearray(data[cursor:cursor+57])
            payload = header[15] * 24 + int.from_bytes(header[18:20], "little") + int.from_bytes(header[20:24], "little")
            body = data[cursor+57:cursor+57+payload]
            updated = body.replace(b'"color":{"type":"int","value":3}', b'"growth":{"type":"int","value":3}')
            header[20:24] = (int.from_bytes(header[20:24], "little") + len(updated) - len(body)).to_bytes(4, "little")
            encoded.extend(header)
            encoded.extend(bytes(9))
            encoded.extend(updated)
            cursor += 57 + payload
        if cursor != len(data):
            raise ValueError("wire fixture length mismatch")
        data = bytes(encoded)
    return "\n".join(data[i:i+16].hex(" ") for i in range(0, len(data), 16)) + "\n"


def outputs(schema):
    """Format both declarations with the standard language formatters."""
    validate(schema)
    rust = subprocess.run(["rustfmt", "--edition", "2024", "--emit", "stdout"],
                          input=rust_declarations(schema), text=True, capture_output=True, check=True).stdout
    go = subprocess.run(["gofmt"], input=go_declarations(schema), text=True,
                        capture_output=True, check=True).stdout
    writer = subprocess.run(["rustfmt", "--edition", "2024", "--emit", "stdout"], input=rust_writer(schema), text=True, capture_output=True, check=True).stdout
    prefix, domain = "\n".join(rust_properties(schema)).split("    /// Returns the complete pinned domain", 1)
    properties = subprocess.run(["rustfmt", "--edition", "2024", "--emit", "stdout"], input=prefix + "}\n", text=True, capture_output=True, check=True).stdout
    domains = subprocess.run(["rustfmt", "--edition", "2024", "--emit", "stdout"], input="use super::BlockProperty;\nimpl BlockProperty {\n    /// Returns the complete pinned domain" + domain, text=True, capture_output=True, check=True).stdout
    return {ROOT / "crates/assets/tests/fixtures/registry-wire.hex": wire_fixture(schema),
            ROOT / "crates/assets/src/facts/property_domains_generated.rs": "// Generated by tools/registrygen/generate_schema.py; edit block-properties-v2193.json.\n" + domains,
            ROOT / "crates/assets/src/facts/properties_generated.rs": "// Generated by tools/registrygen/generate_schema.py; edit block-properties-v2193.json.\n" + properties,
            ROOT / "crates/asset-compiler/src/registry_policy/format_generated.rs": writer,
            ROOT / "crates/assets/src/registry/schema.rs": rust,
            ROOT / "tools/registrygen/schema_generated.go": go}


def main():
    """Regenerate declarations, or fail if the checked-in contract is stale."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    for path, content in outputs(json.loads(SCHEMA.read_text())).items():
        if args.check:
            if not path.exists() or path.read_text() != content:
                parser.exit(1, f"stale registry declaration: {path.relative_to(ROOT)}\n")
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content)


if __name__ == "__main__":
    main()
