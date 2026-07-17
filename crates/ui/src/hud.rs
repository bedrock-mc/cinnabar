use std::{collections::VecDeque, sync::Arc};

pub const MAX_TOASTS: usize = 32;
pub const MAX_TOAST_RETAINED_BYTES: usize = 262_144;
pub const DEFAULT_TITLE_FADE_IN_TICKS: u32 = 10;
pub const DEFAULT_TITLE_STAY_TICKS: u32 = 70;
pub const DEFAULT_TITLE_FADE_OUT_TICKS: u32 = 20;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoundedStat {
    current: u16,
    maximum: u16,
}

impl BoundedStat {
    pub const fn new(current: u16, maximum: u16) -> Option<Self> {
        if maximum == 0 || current > maximum {
            return None;
        }
        Some(Self { current, maximum })
    }

    pub const fn current(self) -> u16 {
        self.current
    }

    pub const fn maximum(self) -> u16 {
        self.maximum
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TitleDurations {
    pub fade_in_ticks: u32,
    pub stay_ticks: u32,
    pub fade_out_ticks: u32,
}

impl Default for TitleDurations {
    fn default() -> Self {
        Self {
            fade_in_ticks: DEFAULT_TITLE_FADE_IN_TICKS,
            stay_ticks: DEFAULT_TITLE_STAY_TICKS,
            fade_out_ticks: DEFAULT_TITLE_FADE_OUT_TICKS,
        }
    }
}

impl TitleDurations {
    pub fn from_wire(fade_in: i32, stay: i32, fade_out: i32) -> Option<Self> {
        Some(Self {
            fade_in_ticks: u32::try_from(fade_in).ok()?,
            stay_ticks: u32::try_from(stay).ok()?,
            fade_out_ticks: u32::try_from(fade_out).ok()?,
        })
    }

    pub const fn total_millis(self) -> u64 {
        (self.fade_in_ticks as u64 + self.stay_ticks as u64 + self.fade_out_ticks as u64) * 50
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimedText {
    pub text: Arc<str>,
    pub fifo_sequence: u64,
    pub started_millis: u64,
    pub expires_millis: u64,
}

impl TimedText {
    pub fn new(
        text: Arc<str>,
        fifo_sequence: u64,
        started_millis: u64,
        durations: TitleDurations,
    ) -> Self {
        Self {
            text,
            fifo_sequence,
            started_millis,
            expires_millis: started_millis.saturating_add(durations.total_millis()),
        }
    }

    pub const fn visible_at(&self, now_millis: u64) -> bool {
        now_millis < self.expires_millis
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Toast {
    pub title: Arc<str>,
    pub message: Arc<str>,
    pub fifo_sequence: u64,
    pub received_millis: u64,
}

impl Toast {
    fn retained_bytes(&self) -> usize {
        self.title.len() + self.message.len()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HudPlayerStatus {
    LoginSuccess,
    FailedClient,
    FailedSpawn,
    PlayerSpawn,
    FailedInvalidTenant,
    FailedVanillaEducation,
    FailedEducationVanilla,
    FailedServerFull,
    FailedEditorVanillaMismatch,
    FailedVanillaEditorMismatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HudViewRole {
    Health,
    Hunger,
    Armor,
    Air,
    Title,
    Subtitle,
    ActionBar,
    ToastTitle,
    ToastMessage,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HudViewNode {
    pub role: HudViewRole,
    pub source_sequence: u64,
    pub text: Arc<str>,
}

#[derive(Clone, Debug, Default)]
pub struct HudStore {
    health: Option<BoundedStat>,
    hunger: Option<BoundedStat>,
    armor: Option<BoundedStat>,
    air: Option<BoundedStat>,
    title: Option<TimedText>,
    subtitle: Option<TimedText>,
    actionbar: Option<TimedText>,
    durations: TitleDurations,
    toasts: VecDeque<Toast>,
    toast_retained_bytes: usize,
    player_status: Option<HudPlayerStatus>,
}

impl HudStore {
    pub const fn health(&self) -> Option<BoundedStat> {
        self.health
    }

    pub const fn hunger(&self) -> Option<BoundedStat> {
        self.hunger
    }

    pub const fn armor(&self) -> Option<BoundedStat> {
        self.armor
    }

    pub const fn air(&self) -> Option<BoundedStat> {
        self.air
    }

    pub const fn title(&self) -> Option<&TimedText> {
        self.title.as_ref()
    }

    pub const fn subtitle(&self) -> Option<&TimedText> {
        self.subtitle.as_ref()
    }

    pub const fn actionbar(&self) -> Option<&TimedText> {
        self.actionbar.as_ref()
    }

    pub fn toasts(&self) -> &VecDeque<Toast> {
        &self.toasts
    }

    pub const fn player_status(&self) -> Option<HudPlayerStatus> {
        self.player_status
    }

    pub const fn durations(&self) -> TitleDurations {
        self.durations
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn set_stats(
        &mut self,
        health: Option<BoundedStat>,
        hunger: Option<BoundedStat>,
        armor: Option<BoundedStat>,
        air: Option<BoundedStat>,
    ) {
        self.health = health;
        self.hunger = hunger;
        self.armor = armor;
        self.air = air;
    }

    pub fn set_health(&mut self, health: Option<BoundedStat>) {
        self.health = health;
    }

    pub fn set_durations(&mut self, durations: TitleDurations) {
        self.durations = durations;
    }

    pub fn set_title(&mut self, text: Arc<str>, fifo_sequence: u64, now_millis: u64) {
        self.title = Some(TimedText::new(
            text,
            fifo_sequence,
            now_millis,
            self.durations,
        ));
    }

    pub fn set_subtitle(&mut self, text: Arc<str>, fifo_sequence: u64, now_millis: u64) {
        self.subtitle = Some(TimedText::new(
            text,
            fifo_sequence,
            now_millis,
            self.durations,
        ));
    }

    pub fn set_actionbar(&mut self, text: Arc<str>, fifo_sequence: u64, now_millis: u64) {
        self.actionbar = Some(TimedText::new(
            text,
            fifo_sequence,
            now_millis,
            self.durations,
        ));
    }

    pub fn clear_titles(&mut self) {
        self.title = None;
        self.subtitle = None;
        self.actionbar = None;
    }

    pub fn reset_titles(&mut self) {
        self.clear_titles();
        self.durations = TitleDurations::default();
    }

    pub fn set_player_status(&mut self, status: HudPlayerStatus) {
        self.player_status = Some(status);
    }

    pub fn push_toast(&mut self, toast: Toast) -> usize {
        let bytes = toast.retained_bytes();
        if bytes > MAX_TOAST_RETAINED_BYTES {
            return 0;
        }
        let mut evicted = 0;
        while self.toasts.len() >= MAX_TOASTS
            || self.toast_retained_bytes + bytes > MAX_TOAST_RETAINED_BYTES
        {
            let Some(removed) = self.toasts.pop_front() else {
                break;
            };
            self.toast_retained_bytes = self
                .toast_retained_bytes
                .saturating_sub(removed.retained_bytes());
            evicted += 1;
        }
        self.toast_retained_bytes += bytes;
        self.toasts.push_back(toast);
        evicted
    }

    pub fn expire(&mut self, now_millis: u64) {
        if self
            .title
            .as_ref()
            .is_some_and(|value| !value.visible_at(now_millis))
        {
            self.title = None;
        }
        if self
            .subtitle
            .as_ref()
            .is_some_and(|value| !value.visible_at(now_millis))
        {
            self.subtitle = None;
        }
        if self
            .actionbar
            .as_ref()
            .is_some_and(|value| !value.visible_at(now_millis))
        {
            self.actionbar = None;
        }
    }

    pub fn view_nodes(&self, now_millis: u64) -> Box<[HudViewNode]> {
        let mut nodes = Vec::with_capacity(7 + self.toasts.len() * 2);
        for (role, value) in [
            (HudViewRole::Health, self.health),
            (HudViewRole::Hunger, self.hunger),
            (HudViewRole::Armor, self.armor),
            (HudViewRole::Air, self.air),
        ] {
            if let Some(value) = value {
                nodes.push(HudViewNode {
                    role,
                    source_sequence: 0,
                    text: Arc::from(format!("{}/{}", value.current(), value.maximum())),
                });
            }
        }
        for (role, value) in [
            (HudViewRole::Title, self.title.as_ref()),
            (HudViewRole::Subtitle, self.subtitle.as_ref()),
            (HudViewRole::ActionBar, self.actionbar.as_ref()),
        ] {
            if let Some(value) = value.filter(|value| value.visible_at(now_millis)) {
                nodes.push(HudViewNode {
                    role,
                    source_sequence: value.fifo_sequence,
                    text: Arc::clone(&value.text),
                });
            }
        }
        for toast in &self.toasts {
            nodes.push(HudViewNode {
                role: HudViewRole::ToastTitle,
                source_sequence: toast.fifo_sequence,
                text: Arc::clone(&toast.title),
            });
            nodes.push(HudViewNode {
                role: HudViewRole::ToastMessage,
                source_sequence: toast.fifo_sequence,
                text: Arc::clone(&toast.message),
            });
        }
        nodes.into_boxed_slice()
    }
}
