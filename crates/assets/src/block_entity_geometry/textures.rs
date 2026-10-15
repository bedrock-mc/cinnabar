//! Native atlas texture roles shared by compilation and model requests.

macro_rules! atlas_textures {
    ($($role:ident => $path:literal,)+) => {
        /// A fixed texture request made by a native model or overlay.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum AtlasTexture { $($role,)+ }
        impl AtlasTexture {
            /// Every fixed native atlas input, in compilation order.
            pub const ALL: &'static [Self] = &[$(Self::$role),+];
            /// Pack-relative texture stem, without an image extension.
            pub const fn path(self) -> &'static str { match self { $(Self::$role => $path,)+ } }
        }
    };
}

atlas_textures! {
    BlocksBellBottom => "textures/blocks/bell_bottom",
    BlocksBellSide => "textures/blocks/bell_side",
    BlocksBellTop => "textures/blocks/bell_top",
    BlocksConduitCage => "textures/blocks/conduit_cage",
    BlocksConduitClosed => "textures/blocks/conduit_closed",
    BlocksConduitOpen => "textures/blocks/conduit_open",
    BlocksConduitWindHorizontal => "textures/blocks/conduit_wind_horizontal",
    BlocksConduitWindVertical => "textures/blocks/conduit_wind_vertical",
    BlocksEndGateway => "textures/blocks/end_gateway",
    BlocksMobSpawner => "textures/blocks/mob_spawner",
    EntityAlex => "textures/entity/alex",
    EntityBeaconBeam => "textures/entity/beacon_beam",
    EntityBellBell => "textures/entity/bell/bell",
    EntityDragonDragon => "textures/entity/dragon/dragon",
    EntityEnchantingTableBook => "textures/entity/enchanting_table_book",
    EntityEndPortal => "textures/entity/end_portal",
    EntityPiglinPiglin => "textures/entity/piglin/piglin",
    EntitySteve => "textures/entity/steve",
    EnvironmentEndPortalColors => "textures/environment/end_portal_colors",
}
