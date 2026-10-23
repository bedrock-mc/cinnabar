//! Typed, bounded envelopes. Runtime bytes never become arbitrary Bedrock packets.

// BEGIN GENERATED EXTENSION CONTRACT
// Generated from crates/server-experience/schema/extension.json; do not edit.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Channel {
    pub id: String,
    pub schema: u16,
    pub direction: Direction,
    pub fields: Vec<Field>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub version: u16,
    pub session: String,
    pub connection: String,
    pub subclient: u8,
    pub bundle: String,
    pub generation: u64,
    pub channel: String,
    pub schema: u16,
    pub sequence: u64,
    pub world_epoch: u64,
    pub payload: Vec<Scalar>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Part {
    pub index: u32,
    pub count: u32,
    pub data: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Fragment {
    pub version: u16,
    pub session: String,
    pub connection: String,
    pub subclient: u8,
    pub bundle: String,
    pub generation: u64,
    pub channel: String,
    pub schema: u16,
    pub sequence: u64,
    pub world_epoch: u64,
    pub fragment: Part,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    ToClient,
    ToServer,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(
    rename_all = "snake_case",
    tag = "type",
    deny_unknown_fields,
    content = "value"
)]
pub enum Scalar {
    Bool(bool),
    Integer(i64),
    Text(String),
    Choice(u16),
    List(Vec<Scalar>),
    Record(Vec<Scalar>),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", tag = "type", deny_unknown_fields)]
pub enum Field {
    Bool {},
    Integer { min: i64, max: i64 },
    Text { max_bytes: u16 },
    Choice { variants: u16 },
    List { item: Box<Field>, max_items: u16 },
    Record { fields: Vec<Field> },
}

impl Field {
    /// Checks container nesting and record field counts before accepting a declaration.
    fn declared(&self, depth: usize) -> bool {
        match self {
            Field::List { item, .. } => depth > 0 && item.declared(depth - 1),
            Field::Record { fields } => {
                depth > 0
                    && fields.len() <= MAX_CHANNEL_FIELDS
                    && fields.iter().all(|field| field.declared(depth - 1))
            }
            _ => true,
        }
    }
    /// Checks the value's type and every schema-defined bound recursively.
    fn admits(&self, value: &Scalar) -> bool {
        match (self, value) {
            (Field::Bool {}, Scalar::Bool(_)) => true,
            (Field::Integer { min, max }, Scalar::Integer(value)) => min <= value && value <= max,
            (Field::Text { max_bytes }, Scalar::Text(value)) => {
                value.len() <= usize::from(*max_bytes)
            }
            (Field::Choice { variants }, Scalar::Choice(value)) => value < variants,
            (Field::List { item, max_items }, Scalar::List(items)) => {
                items.len() <= usize::from(*max_items)
                    && items.iter().all(|value| item.admits(value))
            }
            (Field::Record { fields }, Scalar::Record(values)) => {
                fields.len() == values.len()
                    && fields
                        .iter()
                        .zip(values)
                        .all(|(field, value)| field.admits(value))
            }
            _ => false,
        }
    }
}
impl Channel {
    /// Checks identifiers, field counts and nesting against the shared ceilings.
    pub fn declared(&self) -> bool {
        identifier(&self.id)
            && self.fields.len() <= MAX_CHANNEL_FIELDS
            && self
                .fields
                .iter()
                .all(|field| field.declared(MAX_FIELD_DEPTH))
    }
    /// Checks the record's direction, positional fields and encoded byte ceiling.
    pub fn validate(
        &self,
        payload: &[Scalar],
        direction: Direction,
        max_bytes: usize,
    ) -> Result<()> {
        ensure!(
            self.declared() && self.direction == direction,
            "channel denied"
        );
        ensure!(
            payload.len() == self.fields.len(),
            "record field count mismatch"
        );
        for (field, value) in self.fields.iter().zip(payload) {
            ensure!(field.admits(value), "record field rejected");
        }
        ensure!(
            serde_json::to_vec(payload)?.len() <= max_bytes,
            "payload too large"
        );
        Ok(())
    }
}
// END GENERATED EXTENSION CONTRACT

use crate::rate_limit::RateLimit;
use crate::{
    manifest::identifier,
    negotiation::{Grant, Wire},
    policy::*,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

impl Fragment {
    /// The fragment of `envelope` that carries `part`.
    fn of(envelope: &Envelope, part: Part) -> Self {
        Self {
            version: envelope.version,
            session: envelope.session.clone(),
            connection: envelope.connection.clone(),
            subclient: envelope.subclient,
            bundle: envelope.bundle.clone(),
            generation: envelope.generation,
            channel: envelope.channel.clone(),
            schema: envelope.schema,
            sequence: envelope.sequence,
            world_epoch: envelope.world_epoch,
            fragment: part,
        }
    }

    /// Splits off the part, leaving the header as an envelope with an empty payload.
    fn into_parts(self) -> (Envelope, Part) {
        let header = Envelope {
            version: self.version,
            session: self.session,
            connection: self.connection,
            subclient: self.subclient,
            bundle: self.bundle,
            generation: self.generation,
            channel: self.channel,
            schema: self.schema,
            sequence: self.sequence,
            world_epoch: self.world_epoch,
            payload: Vec::new(),
        };
        (header, self.fragment)
    }
}

/// What one wire v2 carrier message holds.
#[derive(Deserialize)]
#[serde(untagged)]
enum Frame {
    Whole(Envelope),
    Part(Fragment),
}

/// The carrier messages of `envelope` under `wire`: the envelope itself while its payload fits
/// inline, otherwise (since v2) fragments of the payload's JSON in order, each cut at the
/// last character boundary within the fragment limit.
pub fn encode(envelope: &Envelope, wire: &Wire) -> Result<Vec<Vec<u8>>> {
    ensure!(
        envelope.version == wire.version,
        "envelope of another wire version"
    );
    let limits = wire.limits;
    let payload = serde_json::to_string(&envelope.payload)?;
    let max = limits.max_fragment_bytes as usize;
    if payload.len() <= max {
        return Ok(vec![serde_json::to_vec(envelope)?]);
    }
    ensure!(
        wire.version != WIRE_VERSION && payload.len() <= limits.max_message_bytes as usize,
        "payload too large"
    );
    let mut parts = Vec::new();
    let mut rest = payload.as_str();
    while !rest.is_empty() {
        let mut end = max.min(rest.len());
        while !rest.is_char_boundary(end) {
            end -= 1;
        }
        ensure!(end > 0, "fragment limit below one character");
        let (data, tail) = rest.split_at(end);
        parts.push(data);
        rest = tail;
    }
    let count = u32::try_from(parts.len())?;
    parts
        .into_iter()
        .zip(0..)
        .map(|(data, index)| {
            let part = Part {
                index,
                count,
                data: data.to_owned(),
            };
            Ok(serde_json::to_vec(&Fragment::of(envelope, part))?)
        })
        .collect()
}

/// A wire v2 message whose fragments are still arriving.
#[derive(Debug)]
struct Partial {
    /// The first fragment's header, with an empty payload.
    header: Envelope,
    count: u32,
    next: u32,
    data: String,
    /// Carrier bytes of the fragments so far.
    bytes: usize,
}

/// One reliable direction, shared by every bundle in a server session.
#[derive(Debug)]
pub struct Ingress {
    rate: RateLimit,
    next: u64,
    queue: VecDeque<(u64, usize, Envelope)>,
    bytes: usize,
    partial: Option<Partial>,
    failed: bool,
    /// Messages of an undeclared channel schema, possibly a newer revision.
    pub skipped: u64,
    /// Messages of another world epoch than the current one when they came up for dispatch.
    pub stale: u64,
}

impl Ingress {
    /// Takes a server request to advance the logical world epoch on wire v2. Controls share
    /// the envelope rate budget and cannot interrupt a fragmented message.
    pub fn receive_epoch_request(
        &mut self,
        bytes: &[u8],
        now_ms: u64,
        grant: &Grant,
    ) -> Result<Option<u64>> {
        let Ok(crate::session::Control::Epoch {
            session,
            world_epoch,
        }) = serde_json::from_slice(bytes)
        else {
            return Ok(None);
        };
        let result = (|| {
            ensure!(!self.failed, "channel quarantined");
            self.rate.charge(bytes.len(), now_ms)?;
            ensure!(
                grant.wire.version != WIRE_VERSION,
                "world notice requires wire v2"
            );
            ensure!(session == grant.session, "world notice for another session");
            ensure!(world_epoch != 0, "zero world epoch request");
            ensure!(
                self.partial.is_none(),
                "world notice inside a fragmented message"
            );
            Ok(Some(world_epoch))
        })();
        if result.is_err() {
            self.quarantine();
        }
        result
    }

    /// Starts a fresh sequence space after a signed handshake.
    pub fn new(now_ms: u64) -> Self {
        Self {
            rate: RateLimit::new(now_ms),
            next: 1,
            queue: VecDeque::new(),
            bytes: 0,
            partial: None,
            failed: false,
            skipped: 0,
            stale: 0,
        }
    }

    /// Quarantines the optional channel on replay, a sequence gap, a malformed fragment or
    /// overflow.
    pub fn receive<'a>(
        &mut self,
        bytes: &[u8],
        now_ms: u64,
        publication: u64,
        grant: &Grant,
        recipient: impl FnOnce(&str) -> Option<&'a crate::runtime::Capabilities>,
    ) -> Result<()> {
        let result = self.receive_inner(bytes, now_ms, publication, grant, recipient);
        if result.is_err() {
            self.quarantine();
        }
        result
    }

    /// Revokes this direction and releases all queued or partially assembled messages.
    fn quarantine(&mut self) {
        self.failed = true;
        self.queue.clear();
        self.bytes = 0;
        self.partial = None;
    }

    /// Validates identity and schema after charging aggregate ingress cost.
    fn receive_inner<'a>(
        &mut self,
        bytes: &[u8],
        now_ms: u64,
        publication: u64,
        grant: &Grant,
        recipient: impl FnOnce(&str) -> Option<&'a crate::runtime::Capabilities>,
    ) -> Result<()> {
        ensure!(!self.failed, "channel quarantined");
        self.rate.charge(bytes.len(), now_ms)?;
        ensure!(
            grant
                .offer
                .offer
                .scope
                .permissions
                .contains(&crate::manifest::Permission::Messaging),
            "messaging permission denied"
        );
        let limits = grant.wire.limits;
        let (message, size, max_bytes) = if grant.wire.version == WIRE_VERSION {
            let message: Envelope = serde_json::from_slice(bytes)?;
            (message, bytes.len(), limits.max_fragment_bytes)
        } else {
            match serde_json::from_slice(bytes)? {
                Frame::Whole(message) => {
                    ensure!(
                        self.partial.is_none(),
                        "envelope inside a fragmented message"
                    );
                    (message, bytes.len(), limits.max_fragment_bytes)
                }
                Frame::Part(fragment) => match self.reassemble(fragment, bytes.len(), grant)? {
                    Some((message, size)) => (message, size, limits.max_message_bytes),
                    None => return Ok(()),
                },
            }
        };
        self.route(&message, grant)?;
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("sequence exhausted"))?;
        ensure!(
            message.generation == INITIAL_BUNDLE_GENERATION
                && grant
                    .offer
                    .offer
                    .packages
                    .iter()
                    .any(|p| p.id == message.bundle),
            "wrong bundle generation"
        );
        ensure!(
            message.channel.starts_with(&format!("{}.", message.bundle)),
            "foreign channel namespace"
        );
        let recipient =
            recipient(&message.bundle).ok_or_else(|| anyhow::anyhow!("unknown recipient"))?;
        ensure!(
            recipient
                .scope
                .permissions
                .contains(&crate::manifest::Permission::Messaging),
            "recipient messaging permission denied"
        );
        let channels = &recipient.channels;
        ensure!(channels.len() <= MAX_CHANNELS, "channel limit exceeded");
        let Some(channel) = channels
            .iter()
            .find(|c| c.id == message.channel && c.schema == message.schema)
        else {
            self.skipped = self.skipped.saturating_add(1);
            return Ok(());
        };
        channel.validate(&message.payload, Direction::ToClient, max_bytes as usize)?;
        ensure!(
            self.queue.len() < MAX_QUEUE_MESSAGES
                && size <= (limits.max_reassembly_bytes as usize).saturating_sub(self.bytes),
            "reliable queue overflow"
        );
        self.bytes += size;
        self.queue.push_back((publication, size, message));
        Ok(())
    }

    /// Checks the session route and that `message` is the next in sequence.
    fn route(&self, message: &Envelope, grant: &Grant) -> Result<()> {
        ensure!(
            message.version == grant.wire.version
                && message.session == grant.session
                && message.connection == grant.connection
                && message.subclient == grant.subclient,
            "wrong session route"
        );
        ensure!(
            message.sequence == self.next,
            "replay or reliable sequence gap"
        );
        Ok(())
    }

    /// Buffers one fragment, within the per-message cap and with the undelivered messages
    /// within the per-connection budget, and returns the message and its carrier bytes once
    /// the last fragment is in. Fragments come in order, one message at a time.
    fn reassemble(
        &mut self,
        fragment: Fragment,
        size: usize,
        grant: &Grant,
    ) -> Result<Option<(Envelope, usize)>> {
        let limits = grant.wire.limits;
        let (header, part) = fragment.into_parts();
        ensure!(
            !part.data.is_empty() && part.data.len() <= limits.max_fragment_bytes as usize,
            "fragment data size"
        );
        let mut partial = match self.partial.take() {
            Some(partial) => partial,
            None => {
                ensure!(part.count >= 2, "a message of one fragment");
                self.route(&header, grant)?;
                Partial {
                    header: header.clone(),
                    count: part.count,
                    next: 0,
                    data: String::new(),
                    bytes: 0,
                }
            }
        };
        ensure!(
            header == partial.header && part.count == partial.count && part.index == partial.next,
            "fragment out of order"
        );
        partial.data.push_str(&part.data);
        partial.bytes += size;
        partial.next += 1;
        ensure!(
            partial.data.len() <= limits.max_message_bytes as usize,
            "message too large"
        );
        ensure!(
            partial.bytes <= (limits.max_reassembly_bytes as usize).saturating_sub(self.bytes),
            "reassembly budget exceeded"
        );
        if partial.next < partial.count {
            self.partial = Some(partial);
            return Ok(None);
        }
        let mut message = partial.header;
        message.payload = serde_json::from_str(&partial.data)?;
        Ok(Some((message, partial.bytes)))
    }

    /// Inspects the next committed event without consuming it while its helper is busy.
    pub fn peek(&mut self, committed: u64, world_epoch: u64) -> Option<&Envelope> {
        loop {
            let (publication, _, message) = self.queue.front()?;
            if *publication > committed {
                return None;
            }
            if message.world_epoch == world_epoch {
                return self.queue.front().map(|entry| &entry.2);
            }
            let (_, bytes, _) = self.queue.pop_front()?;
            self.bytes -= bytes;
            self.stale = self.stale.saturating_add(1);
        }
    }

    /// Publishes only after preceding world events; old dimension work is discarded.
    pub fn pop(&mut self, committed: u64, world_epoch: u64) -> Option<Envelope> {
        self.peek(committed, world_epoch)?;
        let (_, bytes, message) = self.queue.pop_front()?;
        self.bytes -= bytes;
        Some(message)
    }
}
