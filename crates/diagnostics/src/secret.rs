//! Values that never format or serialize their contents.

use std::fmt;

/// What a [`Secret`] prints and serializes instead of its value.
pub const REDACTED: &str = "[redacted]";

/// A credential or key whose `Debug`, `Display` and `Serialize` output is [`REDACTED`].
///
/// Logging, `{:?}` of an enclosing struct and serialized reports therefore never carry the
/// value; code that needs it calls [`Secret::expose`] at the point of use.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Secret<T>(T);

impl<T> Secret<T> {
    #[must_use]
    pub const fn new(value: T) -> Self {
        Self(value)
    }

    /// The wrapped value, for intentional use such as sending it to its service.
    #[must_use]
    pub const fn expose(&self) -> &T {
        &self.0
    }

    /// Unwraps the value.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> From<T> for Secret<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

impl<T> fmt::Debug for Secret<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(REDACTED)
    }
}

impl<T> fmt::Display for Secret<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(REDACTED)
    }
}

impl<T> serde::Serialize for Secret<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(REDACTED)
    }
}

#[cfg(test)]
mod tests {
    use super::{REDACTED, Secret};

    #[derive(Debug, serde::Serialize)]
    struct Session {
        user: &'static str,
        token: Secret<String>,
    }

    #[test]
    fn formatting_and_serialization_never_show_the_value() {
        let session = Session {
            user: "steve",
            token: Secret::new("eyJhbGciOi.secret".to_owned()),
        };
        let debug = format!("{session:?}");
        let pretty = format!("{session:#?}");
        let display = session.token.to_string();
        let json = serde_json::to_string(&session).unwrap();
        for output in [&debug, &pretty, &display, &json] {
            assert!(!output.contains("eyJhbGciOi"), "{output}");
            assert!(output.contains(REDACTED), "{output}");
        }
        assert_eq!(json, r#"{"user":"steve","token":"[redacted]"}"#);
        let event = format!("{} {:?}", session.token, session.token);
        assert_eq!(event, "[redacted] [redacted]");
    }

    #[test]
    fn expose_and_into_inner_return_the_value() {
        let token = Secret::from(String::from("value"));
        assert_eq!(token.expose(), "value");
        assert_eq!(token.clone().into_inner(), "value");
        assert_eq!(token, Secret::new("value".to_owned()));
    }
}
