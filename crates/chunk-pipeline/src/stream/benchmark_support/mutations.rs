use super::*;

/// A queued packet burst over resident sections, ready for CPU or asynchronous drain timing.
pub struct MutationBurstFixture {
    stream: WorldStream,
    last_sequence: u64,
    initial_sections: BTreeMap<SubChunkKey, Arc<SubChunk>>,
}

impl MutationBurstFixture {
    /// Installs sections and admits a burst that must leave each section changed after draining.
    pub fn new(
        mut stream: WorldStream,
        packets: Vec<Vec<SyncedBlockUpdateEvent>>,
        section: SubChunk,
    ) -> Self {
        let generation = stream.lighting.next_block_generation.wrapping_add(1).max(1);
        stream.lighting.next_block_generation = generation;
        let mut initial_sections = BTreeMap::new();
        for packet in &packets {
            let event = packet.first().expect("benchmark packets contain updates");
            let (key, _) = split_block_update(event.update).unwrap();
            stream
                .authority
                .commit_sub_chunk(key, section.clone())
                .unwrap();
            initial_sections.insert(key, stream.authority.terrain().sub_chunk(key).unwrap());
            stream.resident.insert(key);
            stream.lighting.block_generations.insert(key, generation);
            stream
                .lighting
                .store
                .insert_resident(key, SubChunkLight::uniform(0, 0, generation).unwrap());
            stream.lighting.ownership.insert(
                key,
                LightOwnership {
                    block_generation: generation,
                    light_revision: generation,
                },
            );
            stream.lighting.direct_sky.insert(
                key,
                StoredDirectSky {
                    light_revision: generation,
                    mask: Arc::new(DirectSkyMask::Uniform(false)),
                },
            );
            assert!(
                stream.light_is_current(key),
                "resident fixture light must be current"
            );
        }
        stream.polling = true;
        stream.poll_heavy_guarantee = false;
        stream.poll_deadline = Some(Instant::now());
        let last_sequence = packets.len() as u64;
        for (index, packet) in packets.into_iter().enumerate() {
            stream
                .submit(index as u64 + 1, WorldEvent::SyncedBlockUpdates(packet))
                .unwrap();
        }
        stream.poll_deadline = None;
        Self {
            stream,
            last_sequence,
            initial_sections,
        }
    }

    /// Runs the production preparation jobs on this thread, excluding pool waits and rendering.
    /// Returns the number of worker preparations needed to finish the admitted burst.
    pub fn drain_worker_cpu(&mut self) -> usize {
        let mut jobs = 0;
        loop {
            self.stream.apply_ready();
            let Some(QueuedDecodeJob { queued_at, job }) = self.stream.pending_decode.pop_front()
            else {
                assert_eq!(self.stream.committed_sequence(), self.last_sequence);
                self.assert_changed_sections();
                for &key in self.initial_sections.keys() {
                    assert!(
                        self.stream.revisions.dirty(key).is_some(),
                        "changed section must need a mesh"
                    );
                }
                return jobs;
            };
            self.stream.accept_decode_completion(job.run(queued_at));
            jobs += 1;
        }
    }

    /// Drains the burst through the worker pool, including light, mesh and upload acknowledgment.
    pub fn drain_async(&mut self, camera: [f32; 3]) -> WorldStreamStats {
        self.stream.polling = false;
        let started = Instant::now();
        loop {
            self.stream.poll(camera, 64);
            while let Some(change) = self.stream.pop_mesh_change() {
                let (key, generation, dirty_since) = match change {
                    WorldMeshChange::Upsert {
                        key,
                        generation,
                        dirty_since,
                        ..
                    }
                    | WorldMeshChange::Remove {
                        key,
                        generation,
                        dirty_since,
                        ..
                    } => (key, generation, dirty_since),
                };
                self.stream
                    .acknowledge_mesh_upload(key, generation, dirty_since, Instant::now());
            }
            if self.stream.committed_sequence() == self.last_sequence && work_is_idle(&self.stream)
            {
                self.assert_changed_sections();
                let stages = self.stream.stats.publication_stages;
                assert!(stages.mesh_jobs_completed >= self.initial_sections.len() as u64);
                assert!(stages.mesh_uploads_acknowledged >= self.initial_sections.len() as u64);
                for key in self.initial_sections.keys() {
                    assert!(
                        self.stream.applied_mesh_generations.contains_key(key),
                        "changed section must be uploaded"
                    );
                }
                return self.stream.stats();
            }
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "mutation burst must drain: {:?}",
                self.stream.stats()
            );
            std::thread::yield_now();
        }
    }

    /// Rejects fixtures that drain without changing every installed section's terrain.
    fn assert_changed_sections(&self) {
        assert!(!self.initial_sections.is_empty());
        for (&key, initial) in &self.initial_sections {
            assert_ne!(
                self.stream.authority.terrain().sub_chunk(key).as_ref(),
                Some(initial),
                "burst must change section {key:?}"
            );
        }
    }
}
