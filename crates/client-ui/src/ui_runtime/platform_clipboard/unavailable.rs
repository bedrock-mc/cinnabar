//! A clipboard that is never available, for platforms without a clipboard adapter yet.

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the clipboard is empty")]
    ContentNotAvailable,
    #[error("this platform has no clipboard adapter")]
    Unsupported,
}

pub struct Clipboard;

impl Clipboard {
    pub fn new() -> Result<Self, Error> {
        Err(Error::Unsupported)
    }

    pub fn get_text(&mut self) -> Result<String, Error> {
        Err(Error::Unsupported)
    }

    pub fn set_text(&mut self, _text: String) -> Result<(), Error> {
        Err(Error::Unsupported)
    }
}
