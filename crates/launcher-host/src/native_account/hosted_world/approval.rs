//! Bounded title nonce intent, membership epochs and acknowledged publication state.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use bedrock_services::xbox::mpsd;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io};
use uuid::Uuid;
use zeroize::Zeroizing;

use super::super::world_metadata::xuid;

pub(super) const NONCE_BYTES: usize = 16;

#[derive(Clone, PartialEq, Eq)]
struct MemberIdentity {
    slot: String,
    connection: Option<Uuid>,
    subscription: Option<Uuid>,
}

struct Nonce {
    member: MemberIdentity,
    epoch: u64,
    value: Zeroizing<String>,
}

/// Desired challenges are distinct from the exact revision confirmed by a service response.
pub(super) struct Approvals {
    host: String,
    limit: usize,
    members: BTreeMap<String, MemberIdentity>,
    desired: BTreeMap<String, Nonce>,
    revision: u64,
    acknowledged: Option<u64>,
}

impl Approvals {
    /// Uses the actual finite server cap, reserving its local host before any remote member.
    pub(super) fn new(host: String, max_players: u32) -> io::Result<Self> {
        if !xuid(&host)
            || max_players == 0
            || max_players > bridge::contract::HOSTED_WORLD_MAX_PLAYERS
        {
            return Err(invalid());
        }
        Ok(Self {
            host,
            limit: (max_players - 1) as usize,
            members: BTreeMap::new(),
            desired: BTreeMap::new(),
            revision: 0,
            acknowledged: None,
        })
    }

    /// Replaces membership from a complete snapshot and immediately revokes departed/reentered players.
    /// A changed concrete slot or RTA pair also starts a new epoch even if an absence was not observed.
    pub(super) fn observe(&mut self, description: &mpsd::Description) -> io::Result<()> {
        let result = self.membership(description);
        let members = match result {
            Ok(members) => members,
            Err(error) => {
                self.invalidate();
                return Err(error);
            }
        };
        if members != self.members {
            self.invalidate();
            self.revision = self.revision.checked_add(1).ok_or_else(invalid)?;
            self.desired
                .retain(|xuid, nonce| members.get(xuid) == Some(&nonce.member));
            self.members = members;
        }
        Ok(())
    }

    /// Enumerates only currently joined active members; invite records do not receive a challenge.
    fn membership(
        &self,
        description: &mpsd::Description,
    ) -> io::Result<BTreeMap<String, MemberIdentity>> {
        if description.members.len() > mpsd::MAX_MEMBERS {
            return Err(invalid());
        }
        let mut members = BTreeMap::new();
        let mut host = false;
        for (slot, member) in &description.members {
            let Some(member) = member else {
                continue;
            };
            let Some(owner) = member.xuid().filter(|value| xuid(value)) else {
                continue;
            };
            let system = member
                .properties
                .as_ref()
                .and_then(|section| section.system.as_ref());
            if system
                .and_then(|value| value.get("active"))
                .and_then(Value::as_bool)
                != Some(true)
            {
                continue;
            }
            if owner == self.host {
                if host {
                    return Err(invalid());
                }
                host = true;
                continue;
            }
            if members.len() == self.limit || members.contains_key(owner) {
                return Err(invalid());
            }
            let connection = system.and_then(|value| value.get("connection"));
            let subscription = match system.and_then(|value| value.get("subscription")) {
                None | Some(Value::Null) => None,
                Some(Value::Object(subscription)) => identifier(subscription.get("id"))?,
                _ => return Err(invalid()),
            };
            members.insert(
                owner.to_owned(),
                MemberIdentity {
                    slot: slot.clone(),
                    connection: identifier(connection)?,
                    subscription,
                },
            );
        }
        if !host {
            return Err(invalid());
        }
        Ok(members)
    }

    /// Returns a bounded set of members requiring fresh entropy from the runtime edge.
    pub(super) fn missing(&self) -> Vec<String> {
        self.members
            .keys()
            .filter(|owner| !self.desired.contains_key(*owner))
            .cloned()
            .collect()
    }

    /// Installs edge-supplied cryptographic entropy only for the current unapproved member epoch.
    pub(super) fn issue(&mut self, owner: &str, entropy: &[u8; NONCE_BYTES]) -> io::Result<()> {
        let member = self.members.get(owner).ok_or_else(invalid)?.clone();
        if self.desired.contains_key(owner) {
            return Err(invalid());
        }
        self.desired.insert(
            owner.to_owned(),
            Nonce {
                member,
                epoch: self.revision,
                value: Zeroizing::new(URL_SAFE_NO_PAD.encode(entropy)),
            },
        );
        self.invalidate();
        Ok(())
    }

    /// Builds the complete current desired nonce map; incomplete entropy cannot be published.
    pub(super) fn document(&self) -> io::Result<Value> {
        if self.desired.len() != self.members.len() {
            return Err(invalid());
        }
        Ok(Value::Object(
            self.desired
                .iter()
                .map(|(owner, nonce)| (owner.clone(), Value::String(nonce.value.to_string())))
                .collect(),
        ))
    }

    /// Accepts only the exact current document at the revision returned by an acknowledged service write/read.
    pub(super) fn acknowledge(&mut self, revision: u64, actual: &Value) -> io::Result<()> {
        if revision != self.revision || actual != &self.document()? {
            self.invalidate();
            return Err(invalid());
        }
        self.acknowledged = Some(revision);
        Ok(())
    }

    /// Denies all new approvals while a write or complete-snapshot proof is unresolved.
    pub(super) fn invalidate(&mut self) {
        self.acknowledged = None;
    }

    /// Captures the immutable membership/nonce revision for one conditional property write.
    pub(super) fn revision(&self) -> u64 {
        self.revision
    }

    /// Reports acknowledgment of this exact current complete desired revision, without inferring it from intent.
    pub(super) fn acknowledged(&self) -> bool {
        self.acknowledged == Some(self.revision) && self.desired.len() == self.members.len()
    }

    /// Checks title nonce policy only; the transport owner must already authenticate this Login XUID.
    pub(super) fn accepts(&self, owner: &str, nonce: &str) -> bool {
        self.acknowledged == Some(self.revision)
            && owner != self.host
            && self.desired.get(owner).is_some_and(|entry| {
                entry.value.as_str() == nonce && self.members.get(owner) == Some(&entry.member)
            })
    }

    /// Captures only this member's acknowledged original nonce epoch.
    pub(super) fn member_epoch(&self, owner: &str) -> Option<u64> {
        self.desired.get(owner).and_then(|entry| {
            (self.acknowledged() && self.members.get(owner) == Some(&entry.member))
                .then_some(entry.epoch)
        })
    }

    /// Retains a staying member's epoch while requiring acknowledgment of the current whole document.
    pub(super) fn rechecks(&self, owner: &str, epoch: u64, digest: &[u8; 32]) -> io::Result<bool> {
        let entry = self.desired.get(owner).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "host approval member departed")
        })?;
        if entry.epoch != epoch
            || self.members.get(owner) != Some(&entry.member)
            || <[u8; 32]>::from(Sha256::digest(entry.value.as_bytes())) != *digest
        {
            return Ok(false);
        }
        if !self.acknowledged() {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "host approval document awaits acknowledgment",
            ));
        }
        Ok(true)
    }
}

/// Accepts absent binding hints without confusing a supplied malformed UUID with absence.
fn identifier(value: Option<&Value>) -> io::Result<Option<Uuid>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Uuid::parse_str(value)
            .ok()
            .filter(|id| !id.is_nil())
            .map(Some)
            .ok_or_else(invalid),
        _ => Err(invalid()),
    }
}

/// Keeps member identifiers and admission challenges out of validation diagnostics.
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid hosted-world membership or approval state",
    )
}

#[cfg(test)]
mod tests;
