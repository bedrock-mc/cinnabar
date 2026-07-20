pub(in crate::compiler) fn exact_tagged_string(value: &serde_json::Value) -> Option<&str> {
    let object = value.as_object()?;
    if object.len() != 2 || object.get("type")?.as_str()? != "string" {
        return None;
    }
    object.get("value")?.as_str()
}

pub(in crate::compiler) fn exact_tagged_byte(value: &serde_json::Value, maximum: u8) -> Option<u8> {
    let object = value.as_object()?;
    if object.len() != 2 || object.get("type")?.as_str()? != "byte" {
        return None;
    }
    let value = u8::try_from(object.get("value")?.as_u64()?).ok()?;
    (value <= maximum).then_some(value)
}

pub(in crate::compiler) fn exact_tagged_int(
    value: &serde_json::Value,
    maximum: u32,
) -> Option<u32> {
    let object = value.as_object()?;
    if object.len() != 2 || object.get("type")?.as_str()? != "int" {
        return None;
    }
    let value = u32::try_from(object.get("value")?.as_u64()?).ok()?;
    (value <= maximum).then_some(value)
}
