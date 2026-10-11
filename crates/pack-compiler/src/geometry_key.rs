/// Splits a legacy geometry name and optional parent, rejecting malformed inheritance keys.
pub(crate) fn legacy_key(key: &str) -> Option<(&str, Option<&str>)> {
    match key.split_once(':') {
        Some((identifier, parent))
            if identifier.starts_with("geometry.")
                && parent.starts_with("geometry.")
                && !parent.contains(':') =>
        {
            Some((identifier, Some(parent)))
        }
        Some(_) => None,
        None => Some((key, None)),
    }
}
