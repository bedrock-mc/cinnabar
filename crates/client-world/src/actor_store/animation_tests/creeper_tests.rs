//! Creeper swell sampled between ticks, from fuse packets through to rig layers.
use std::sync::Arc;

use assets::{MolangOp, RuntimeEntityAssets};
use protocol::{ActorKind, ActorMetadataValue};

mod fixtures_tests;
use fixtures_tests::*;

mod clock_tests;
mod effects_tests;
mod installed_tests;
mod packet_tests;
mod pose_tests;
mod selection_tests;
