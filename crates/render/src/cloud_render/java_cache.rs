//! Retains bounded speculative meshes for the optional world cloud layer.

use assets::AtmosphereTexture;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on};
use meshing::{cloud_viewport::ViewportCloudQuad, java_clouds::JavaCloudViewport};
use std::{collections::HashMap, sync::Arc};

enum Job {
    Pending(Task<Arc<[ViewportCloudQuad]>>),
    Ready(Arc<[ViewportCloudQuad]>),
}

#[derive(Default)]
pub(super) struct JavaCloudCache {
    identity: Option<[u8; 32]>,
    texture: Option<Arc<AtmosphereTexture>>,
    jobs: HashMap<JavaCloudViewport, Job>,
}

impl JavaCloudCache {
    /// Returns the exact requested window and prefetches its four adjacent cells.
    pub fn get(
        &mut self,
        identity: [u8; 32],
        texture: &AtmosphereTexture,
        viewport: JavaCloudViewport,
    ) -> Arc<[ViewportCloudQuad]> {
        if self.identity != Some(identity) {
            self.jobs.clear();
            self.texture = Some(Arc::new(texture.clone()));
            self.identity = Some(identity);
        }
        self.jobs.retain(|key, _| key.is_neighbour_of(viewport));
        for (x, z) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
            let key = viewport.shifted(x, z);
            self.jobs.entry(key).or_insert_with(|| {
                let texture = Arc::clone(self.texture.as_ref().expect("cloud texture installed"));
                Job::Pending(
                    AsyncComputeTaskPool::get()
                        .spawn(async move { Arc::from(meshing::java_clouds::mesh(&texture, key)) }),
                )
            });
        }
        if let Some(records) = self.ready(identity, viewport) {
            return records;
        }
        if let Job::Pending(task) = self
            .jobs
            .remove(&viewport)
            .expect("requested cloud window started")
        {
            let records = block_on(task);
            self.jobs.insert(viewport, Job::Ready(Arc::clone(&records)));
            records
        } else {
            unreachable!("ready jobs are borrowed before removal")
        }
    }

    /// Returns retained records without waiting for a worker or cloning its allocation.
    pub fn ready(
        &self,
        identity: [u8; 32],
        viewport: JavaCloudViewport,
    ) -> Option<Arc<[ViewportCloudQuad]>> {
        if self.identity != Some(identity) {
            return None;
        }
        match self.jobs.get(&viewport)? {
            Job::Ready(records) => Some(Arc::clone(records)),
            Job::Pending(_) => None,
        }
    }
}
