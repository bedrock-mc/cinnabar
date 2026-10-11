use super::*;

#[test]
fn replacing_a_pack_preserves_the_pool_and_discards_stale_completion() {
    let encoded = assets::encode_sound_bank(b"{}", b"{}", b"{}", &[]).unwrap();
    let mut bank = SoundBank::from_parts(
        SoundBankIndex::decode_prefix(&encoded).unwrap(),
        SoundEventTables::default(),
        None,
    );
    let (jobs, _job_queue) = sync_channel(MAX_QUEUED_DECODES);
    let (results, done) = sync_channel(DECODE_WORKERS);
    let generation = Arc::new(AtomicU64::new(0));
    bank.decoder = Some(Decoder {
        jobs,
        done: Mutex::new(done),
        generation: Arc::clone(&generation),
        encoded_bytes: Arc::new(AtomicUsize::new(0)),
        server: Arc::new(Mutex::new((0, None))),
    });
    bank.install_server(Some(Arc::new(ServerSoundPack::default())));
    let decoder = bank.decoder.as_ref().expect("one pool survives reload");
    assert!(Arc::ptr_eq(&decoder.generation, &generation));
    let epoch = generation.load(Ordering::Acquire);
    assert_ne!(epoch, 0);
    bank.in_flight.insert("sounds/shared".into(), (false, 0));
    let pcm = |value| Pcm {
        channels: 1,
        rate: 8000,
        samples: vec![value; 2].into(),
    };
    results
        .send((0, "sounds/shared".into(), Some(pcm(1234))))
        .unwrap();
    results
        .send((epoch, "sounds/shared".into(), Some(pcm(-2345))))
        .unwrap();
    bank.poll();
    let PcmLookup::Ready(loaded) = bank.lookup("sounds/shared", false) else {
        panic!("current generation completed");
    };
    assert_eq!(loaded.samples.first(), Some(&-2345));
    assert!(!bank.is_decoding("sounds/shared"));
}

#[test]
fn queued_input_budget_remains_held_across_pack_generations() {
    let decoder = Decoder::spawn(0, None).unwrap();
    let held = decoder.reserve(MAX_QUEUED_DECODE_BYTES).unwrap();
    decoder.generation.store(1, Ordering::Release);
    assert!(decoder.reserve(1).is_none());
    drop(held);
    assert!(decoder.reserve(MAX_QUEUED_DECODE_BYTES).is_some());
}

#[test]
fn stale_queued_sounds_do_not_retain_previous_pack_archives() {
    let encoded = assets::encode_sound_bank(b"{}", b"{}", b"{}", &[]).unwrap();
    let mut bank = SoundBank::from_parts(
        SoundBankIndex::decode_prefix(&encoded).unwrap(),
        SoundEventTables::default(),
        None,
    );
    let (jobs, _job_queue) = sync_channel(MAX_QUEUED_DECODES);
    let (_results, done) = sync_channel(DECODE_WORKERS);
    bank.decoder = Some(Decoder {
        jobs,
        done: Mutex::new(done),
        generation: Arc::new(AtomicU64::new(0)),
        encoded_bytes: Arc::new(AtomicUsize::new(0)),
        server: Arc::new(Mutex::new((0, None))),
    });
    let mut retired = Vec::new();
    for _ in 0..4 {
        let mut pack = ServerSoundPack::default();
        pack.files.insert("sounds/pending".into());
        let pack = Arc::new(pack);
        retired.push(Arc::downgrade(&pack));
        bank.install_server(Some(pack));
        assert!(matches!(
            bank.lookup("sounds/pending", false),
            PcmLookup::Pending
        ));
    }
    for pack in &retired[..retired.len() - 1] {
        assert!(
            pack.upgrade().is_none(),
            "queued jobs must not own retired archives"
        );
    }
    assert!(retired.last().unwrap().upgrade().is_some());
}
