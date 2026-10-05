//! Records the files a compiler actually consumes, including missing fallback candidates.

use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

/// A content read or directory listing that can affect a compiled subscriber.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PackDependency {
    File {
        path: String,
        limit: u64,
    },
    Directory(String),
    /// Names matching the suffixes under the prefix; unrelated files do not invalidate it.
    DirectoryWithSuffixes {
        prefix: String,
        suffixes: Vec<String>,
    },
    /// Names and bytes of every file under the prefix, for a subscriber that reads lazily.
    Contents(String),
}

/// Shared only within one compilation; clones record into the same dependency set.
#[derive(Clone, Debug, Default)]
pub struct PackDependencies(Arc<Mutex<BTreeSet<PackDependency>>>);

impl PackDependencies {
    /// Returns an immutable copy without retaining the compilation's lock.
    pub fn snapshot(&self) -> BTreeSet<PackDependency> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    /// Restores the inputs retained alongside a reused compilation result.
    pub fn extend(&self, inputs: impl IntoIterator<Item = PackDependency>) {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .extend(inputs);
    }

    /// Records an attempted read, even when the stack contains no matching file.
    pub(crate) fn file(&self, path: &str, limit: u64) {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(PackDependency::File {
                path: path.to_owned(),
                limit,
            });
    }

    /// Directory dependencies track names and order separately from file contents.
    pub(crate) fn directory(&self, prefix: &str) {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(PackDependency::Directory(prefix.to_owned()));
    }

    pub(crate) fn contents(&self, prefix: &str) {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(PackDependency::Contents(prefix.to_owned()));
    }

    /// Records a directory filter with sorted, unique suffixes for stable reload checks.
    pub(crate) fn directory_with_suffixes(&self, prefix: &str, suffixes: &[&str]) {
        let mut suffixes: Vec<String> =
            suffixes.iter().map(|suffix| (*suffix).to_owned()).collect();
        suffixes.sort();
        suffixes.dedup();
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(PackDependency::DirectoryWithSuffixes {
                prefix: prefix.to_owned(),
                suffixes,
            });
    }
}

impl PackDependency {
    /// Appends `inputs` in a self-delimiting form that [`Self::decode_set`] reads back.
    pub fn encode_set(inputs: &BTreeSet<Self>, out: &mut Vec<u8>) {
        fn text(out: &mut Vec<u8>, value: &str) {
            out.extend_from_slice(&(value.len() as u32).to_le_bytes());
            out.extend_from_slice(value.as_bytes());
        }
        out.extend_from_slice(&(inputs.len() as u32).to_le_bytes());
        for input in inputs {
            match input {
                Self::File { path, limit } => {
                    out.push(0);
                    text(out, path);
                    out.extend_from_slice(&limit.to_le_bytes());
                }
                Self::Directory(prefix) => {
                    out.push(1);
                    text(out, prefix);
                }
                Self::DirectoryWithSuffixes { prefix, suffixes } => {
                    out.push(2);
                    text(out, prefix);
                    out.extend_from_slice(&(suffixes.len() as u32).to_le_bytes());
                    for suffix in suffixes {
                        text(out, suffix);
                    }
                }
                Self::Contents(prefix) => {
                    out.push(3);
                    text(out, prefix);
                }
            }
        }
    }

    /// Reads a set written by [`Self::encode_set`] and returns the bytes after it.
    pub fn decode_set(bytes: &[u8]) -> Option<(BTreeSet<Self>, &[u8])> {
        let mut reader = Reader(bytes);
        let mut inputs = BTreeSet::new();
        for _ in 0..reader.u32()? {
            let input = match reader.take(1)?[0] {
                0 => Self::File {
                    path: reader.text()?,
                    limit: u64::from_le_bytes(reader.take(8)?.try_into().ok()?),
                },
                1 => Self::Directory(reader.text()?),
                2 => Self::DirectoryWithSuffixes {
                    prefix: reader.text()?,
                    suffixes: (0..reader.u32()?)
                        .map(|_| reader.text())
                        .collect::<Option<_>>()?,
                },
                3 => Self::Contents(reader.text()?),
                _ => return None,
            };
            inputs.insert(input);
        }
        Some((inputs, reader.0))
    }
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, length: usize) -> Option<&'a [u8]> {
        let (head, rest) = self.0.split_at_checked(length)?;
        self.0 = rest;
        Some(head)
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn text(&mut self) -> Option<String> {
        let length = self.u32()? as usize;
        String::from_utf8(self.take(length)?.to_vec()).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_encoded_set_round_trips_and_truncation_is_rejected() {
        let inputs = BTreeSet::from([
            PackDependency::File {
                path: "entity/a.json".into(),
                limit: 7,
            },
            PackDependency::Directory("textures/".into()),
            PackDependency::DirectoryWithSuffixes {
                prefix: "models/".into(),
                suffixes: vec![".json".into(), ".geo.json".into()],
            },
            PackDependency::Contents("sounds/".into()),
        ]);
        let mut bytes = Vec::new();
        PackDependency::encode_set(&inputs, &mut bytes);
        bytes.extend_from_slice(b"tail");
        let (decoded, rest) = PackDependency::decode_set(&bytes).unwrap();
        assert_eq!((decoded, rest), (inputs, &b"tail"[..]));
        assert!(PackDependency::decode_set(&bytes[..bytes.len() - 9]).is_none());
    }
}
