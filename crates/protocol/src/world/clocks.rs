use valentine::bedrock::version::v1_26_51::SyncWorldClocksPacketData;

/// The vanilla clock consumed by Level::getTime and the atmosphere renderer.
///
/// The vanilla initializer constructs this exact
/// name; Level::getTime uses the resulting hashed string.
pub const OVERWORLD_CLOCK_NAME: &str = "minecraft:overworld";

/// Native HashedString key for the built-in daylight clock. The current
/// initializer assigns this key alongside OVERWORLD_CLOCK_NAME;
/// registerWorldClock pre-registers it and Level::getTime's
/// lookup searches that ID directly, not packet string names.
pub const OVERWORLD_CLOCK_ID: u64 = 0x63d7_ede7_3c38_f916;

/// One server-authored clock registration. Preserve the transmitted ID;
/// a string name must not substitute for native hashed-ID lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldClockDefinition {
    pub id: u64,
    pub time: i32,
    pub paused: bool,
}

/// One update addressed to an already registered server clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldClockState {
    pub id: u64,
    pub time: i32,
    pub paused: bool,
}

/// The SyncWorldClocks payloads used by the client clock consumer.
///
/// Native RegistryClient initialization upserts registrations; it does not
/// clear clocks omitted by a later initialization payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldClockUpdateEvent {
    Initialize(WorldClockDefinition),
    Sync(WorldClockState),
}

pub(super) fn normalize_world_clocks(
    data: SyncWorldClocksPacketData,
) -> Vec<WorldClockUpdateEvent> {
    match data {
        SyncWorldClocksPacketData::InitializeRegistryData(data) => data
            .clock_data
            .into_iter()
            .map(|clock| {
                WorldClockUpdateEvent::Initialize(WorldClockDefinition {
                    id: clock.id,
                    time: clock.time,
                    paused: clock.is_paused,
                })
            })
            .collect(),
        SyncWorldClocksPacketData::SyncStateData(data) => data
            .clock_data
            .into_iter()
            .map(|clock| {
                WorldClockUpdateEvent::Sync(WorldClockState {
                    id: clock.clock_id,
                    time: clock.time,
                    paused: clock.is_paused,
                })
            })
            .collect(),
        // Marker registration is decoded completely, but Cinnabar currently
        // has no world-clock event/script consumer. It does not change time
        // or pause state in the native handlers.
        SyncWorldClocksPacketData::AddTimeMarkerData(_)
        | SyncWorldClocksPacketData::RemoveTimeMarkerData(_) => Vec::new(),
    }
}
