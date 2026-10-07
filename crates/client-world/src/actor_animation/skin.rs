//! Player skins that carry their own model: the player's animations drive the skin's bones by
//! name, so the rig poses the skin geometry instead of the default humanoid.
use assets::{SkinGeometry, parse_skin_geometry};
use protocol::SkinGeometrySource;

use super::{pose::LocalDelta, *};

#[derive(Debug)]
pub(super) enum SkinModel {
    Parsed(SkinSkeleton),
    /// The default geometry applies to this source (none named, or it failed to resolve).
    Default(Arc<SkinGeometrySource>),
}

#[derive(Debug)]
pub(super) struct SkinSkeleton {
    source: Arc<SkinGeometrySource>,
    pub(super) geometry: Arc<SkinGeometry>,
    pub(super) bones: Vec<RuntimeBone>,
    pub(super) names: Vec<Box<str>>,
    pub(super) layers: Vec<super::skin_layers::SkinLayerSkeleton>,
    /// Default-rig bone whose animation each skin bone takes, matched by name.
    driver: Vec<Option<usize>>,
    /// Geometry binding `driver` was matched against.
    driver_binding: usize,
}

impl SkinModel {
    fn source(&self) -> &Arc<SkinGeometrySource> {
        match self {
            Self::Parsed(skeleton) => &skeleton.source,
            Self::Default(source) => source,
        }
    }
}

impl ActorRigState {
    pub(super) fn skin_skeleton(&self) -> Option<&SkinSkeleton> {
        match &self.skin {
            Some(SkinModel::Parsed(skeleton)) => Some(skeleton),
            _ => None,
        }
    }

    /// Bones the rig poses: the skin's when it has a model.
    pub(super) fn posed_bones(&self) -> &[RuntimeBone] {
        self.skin_skeleton()
            .map_or(&self.bones, |skeleton| &skeleton.bones)
    }

    /// Names of the bones the rig poses: the skin's when it has a model.
    pub(super) fn posed_bone_names(&self) -> &[Box<str>] {
        self.skin_skeleton()
            .map_or(&self.bone_names, |skeleton| &skeleton.names)
    }

    /// Composes the default rig's animation deltas onto the posed skeleton.
    pub(super) fn compose(&self, local: &[LocalDelta]) -> Option<Vec<BoneTransform>> {
        match self.skin_skeleton() {
            Some(skeleton) => {
                let local = skeleton
                    .driver
                    .iter()
                    .map(|driver| {
                        driver
                            .and_then(|index| local.get(index).copied())
                            .unwrap_or_default()
                    })
                    .collect::<Vec<_>>();
                compose_pose(&skeleton.bones, &local)
            }
            None => compose_pose(&self.bones, local),
        }
    }

    /// Rematches the skin's bones to the default rig after the rig's geometry changed.
    pub(super) fn refresh_skin_drivers(&mut self) {
        let (bone_names, binding) = (&self.bone_names, self.geometry_binding);
        if let Some(SkinModel::Parsed(skeleton)) = &mut self.skin
            && skeleton.driver_binding != binding
        {
            skeleton.driver = drivers(&skeleton.names, bone_names);
            skeleton.driver_binding = binding;
            self.rest_on_posed_skeleton();
        }
    }

    /// Resets the stored poses to the posed skeleton's rest, after its bones changed.
    pub(super) fn rest_on_posed_skeleton(&mut self) {
        let rest = match self.skin_skeleton() {
            Some(skeleton) => compose_pose(&skeleton.bones, &[]),
            None => compose_pose(&self.bones, &[]),
        };
        if let Some(rest) = rest {
            self.previous.clone_from(&rest);
            self.current.clone_from(&rest);
            self.rest = rest;
            self.reset_pending = true;
            self.rest_reset_pending = true;
            self.world_body = None;
        }
    }
}

fn drivers(skin: &[Box<str>], rig: &[Box<str>]) -> Vec<Option<usize>> {
    skin.iter()
        .map(|name| rig.iter().position(|candidate| candidate == name))
        .collect()
}

/// Brings the rig's skin model in line with the skin the actor now wears. Returns whether a
/// source was rejected.
pub(super) fn sync_skin(
    state: &mut ActorRigState,
    source: Option<&Arc<SkinGeometrySource>>,
    assets: &RuntimeEntityAssets,
) -> bool {
    let unchanged = match (&state.skin, source) {
        (None, None) => true,
        (Some(model), Some(source)) => {
            Arc::ptr_eq(model.source(), source) || model.source().as_ref() == source.as_ref()
        }
        _ => false,
    };
    if unchanged {
        return false;
    }
    let had_skeleton = state.skin_skeleton().is_some();
    let mut rejected = false;
    state.skin = source.map(|source| {
        let parsed =
            parse_skin_geometry(&source.resource_patch, &source.geometry_data).map(|geometry| {
                geometry.or_else(|| {
                    let name = assets::skin_geometry_name(&source.resource_patch)?;
                    let geometry = assets
                        .geometries()
                        .iter()
                        .find(|geometry| geometry.identifier.eq_ignore_ascii_case(&name))?;
                    SkinGeometry::from_catalog(geometry)
                })
            });
        match parsed {
            Ok(Some(geometry)) => match skeleton(&geometry.bones) {
                Some((bones, names)) => SkinModel::Parsed(SkinSkeleton {
                    source: Arc::clone(source),
                    driver: drivers(&names, &state.bone_names),
                    driver_binding: state.geometry_binding,
                    geometry: Arc::new(geometry),
                    layers: super::skin_layers::parse(source),
                    bones,
                    names,
                }),
                None => {
                    rejected = true;
                    SkinModel::Default(Arc::clone(source))
                }
            },
            Ok(None) => SkinModel::Default(Arc::clone(source)),
            Err(_) => {
                rejected = true;
                SkinModel::Default(Arc::clone(source))
            }
        }
    });
    if had_skeleton || state.skin_skeleton().is_some() {
        state.rest_on_posed_skeleton();
    }
    rejected
}
