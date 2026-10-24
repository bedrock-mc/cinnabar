//! Shared sound categories for settings and the mixer.

macro_rules! audio_categories {
    ($($category:ident $(=> $name:literal)?;)+) => {
        /// A shared sound bucket.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum AudioCategory { $($category,)+ }

        impl AudioCategory {
            pub const ALL: &[Self] = &[$(Self::$category,)+];
            pub const COUNT: usize = Self::ALL.len();

            /// Maps a sound definition's category; unknown names use the sound bucket.
            pub fn from_definition(category: Option<&str>) -> Self {
                match category { $($(Some($name) => Self::$category,)?)+ _ => Self::Sound }
            }
        }
    }
}

audio_categories! {
    Master;
    Music => "music";
    Sound;
    Ambient => "ambient";
    Players => "player";
    Blocks => "block";
    Hostile => "hostile";
    Neutral => "neutral";
    Records => "record";
    Weather => "weather";
    Ui => "ui";
}

impl AudioCategory {
    /// Returns the dense mixer index for this category.
    pub const fn slot(self) -> usize {
        self as usize
    }
}
