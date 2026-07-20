#[test]
fn make_client_rebuilds_only_a_missing_or_stale_asset_blob() {
    let makefile = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("Makefile"),
    )
    .unwrap()
    .replace("\r\n", "\n");

    for contract in [
        "LIGHT_REGISTRY ?= crates/assets/data/block-light-registry-v1001.bin",
        concat!(
            "ASSET_COMPILER_INPUTS := Cargo.toml Cargo.lock crates/assets/Cargo.toml ",
            "crates/asset-compiler/Cargo.toml Makefile $(wildcard crates/assets/src/*.rs) ",
            "$(wildcard crates/assets/src/*/*.rs) $(wildcard crates/asset-compiler/src/*.rs) ",
            "$(wildcard crates/asset-compiler/src/*/*.rs) ",
            "$(wildcard crates/asset-compiler/src/*/*/*.rs)"
        ),
        concat!(
            "$(ASSET_BLOB): $(PACK_SENTINEL) $(ASSET_COMPILER_INPUTS) $(BLOCK_REGISTRY) ",
            "$(LIGHT_REGISTRY) $(BIOME_REGISTRY)"
        ),
        "assets: $(ASSET_BLOB)",
        "client: assets physics-assets",
        "\t$(CLIENT_RUN)",
        "--light-registry \"$(LIGHT_REGISTRY)\"",
    ] {
        assert!(
            makefile.contains(contract),
            "missing Makefile contract: {contract}"
        );
    }

    let phony = makefile
        .lines()
        .find(|line| line.starts_with(".PHONY:"))
        .expect("Makefile has a .PHONY declaration");
    assert!(!phony.split_whitespace().any(|word| word == "$(ASSET_BLOB)"));
}

#[test]
fn make_client_passes_no_vsync_only_when_requested() {
    let makefile = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("Makefile"),
    )
    .unwrap()
    .replace("\r\n", "\n");

    assert!(makefile.contains("NO_VSYNC ?= 0"));
    assert!(makefile.contains("$(if $(filter 1,$(NO_VSYNC)),--no-vsync)"));
}

#[test]
fn make_assets_and_client_refresh_the_atmosphere_blob_and_report() {
    let makefile = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("Makefile"),
    )
    .unwrap()
    .replace("\r\n", "\n");

    for contract in [
        "VANILLA_SOURCE_MANIFEST ?= assets/vanilla-source.json",
        "ATMOSPHERE_BLOB ?= .local/assets/compiled/vanilla-v1.mcbeatm",
        "ATMOSPHERE_REPORT ?= .local/assets/compiled/atmosphere-assets.json",
        "CINNABAR_CLOUDS_PNG ?=",
        "Set CINNABAR_CLOUDS_PNG to the exact local-only Bedrock 1.26.33.1 clouds.png",
        "CLOUDS_OVERRIDE_PREREQUISITE = FORCE_CINNABAR_CLOUDS_OVERRIDE",
        "$(VANILLA_SOURCE_MANIFEST) $(CLOUDS_OVERRIDE_PREREQUISITE)",
        "FORCE_CINNABAR_CLOUDS_OVERRIDE:",
        concat!(
            "$(ATMOSPHERE_BLOB): $(ASSET_BLOB) $(ASSET_COMPILER_INPUTS) ",
            "$(VANILLA_SOURCE_MANIFEST)"
        ),
        "$(ATMOSPHERE_REPORT): $(ATMOSPHERE_BLOB)",
        "\t$(RUN_IF_ASSET_REPORT_STALE) || $(ATMOSPHERE_COMPILE)",
        "\t$(ATMOSPHERE_COMPILE)",
        "atmosphere-assets: $(ATMOSPHERE_BLOB) $(ATMOSPHERE_REPORT)",
        "assets: $(ASSET_BLOB) $(ATMOSPHERE_BLOB) $(ATMOSPHERE_REPORT)",
        "--source-manifest \"$(VANILLA_SOURCE_MANIFEST)\"",
        "$(if $(strip $(CINNABAR_CLOUDS_PNG)),--clouds-override \"$(CINNABAR_CLOUDS_PNG)\")",
        "--out \"$(ATMOSPHERE_BLOB)\" --report \"$(ATMOSPHERE_REPORT)\"",
    ] {
        assert!(
            makefile.contains(contract),
            "missing atmosphere Makefile contract: {contract}"
        );
    }

    let phony = makefile
        .lines()
        .find(|line| line.starts_with(".PHONY:"))
        .expect("Makefile has a .PHONY declaration");
    assert!(
        phony
            .split_whitespace()
            .any(|word| word == "atmosphere-assets")
    );
    assert!(
        !phony
            .split_whitespace()
            .any(|word| word == "$(ATMOSPHERE_BLOB)" || word == "$(ATMOSPHERE_REPORT)")
    );
    assert!(!makefile.contains("$(ATMOSPHERE_BLOB) $(ATMOSPHERE_REPORT):"));
    assert_eq!(
        makefile
            .lines()
            .filter(|line| line.starts_with('\t') && line.contains("$(ATMOSPHERE_COMPILE)"))
            .count(),
        2,
        "blob and missing-report recovery must use one shared producer command"
    );

    for (report, carrier, compiler) in [
        ("ATMOSPHERE_REPORT", "ATMOSPHERE_BLOB", "ATMOSPHERE_COMPILE"),
        ("ENTITY_ASSET_REPORT", "ENTITY_ASSET_BLOB", "ENTITY_ASSET_COMPILE"),
        ("FONT_ASSET_REPORT", "FONT_ASSET_BLOB", "FONT_ASSET_COMPILE"),
        ("HUD_ASSET_REPORT", "HUD_ASSET_BLOB", "HUD_ASSET_COMPILE"),
    ] {
        let contract = format!(
            "$({report}): $({carrier})\n\t$(RUN_IF_ASSET_REPORT_STALE) || $({compiler})"
        );
        assert!(
            makefile.contains(&contract),
            "missing cross-platform report recovery contract: {contract}"
        );
    }
}

#[test]
fn make_assets_and_client_refresh_the_entity_carrier_and_report() {
    let makefile = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("Makefile"),
    )
    .unwrap()
    .replace("\r\n", "\n");

    for contract in [
        "ENTITY_ASSET_BLOB ?= .local/assets/compiled/vanilla-v1.mcbeent",
        "ENTITY_ASSET_REPORT ?= .local/assets/compiled/entity-assets.json",
        concat!(
            "ENTITY_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- ",
            "entity-assets --pack \"$(PACK_DIR)\" --source-manifest \"$(VANILLA_SOURCE_MANIFEST)\" ",
            "--out \"$(ENTITY_ASSET_BLOB)\" --report \"$(ENTITY_ASSET_REPORT)\""
        ),
        "entity-assets: $(ENTITY_ASSET_BLOB) $(ENTITY_ASSET_REPORT)",
        "$(ENTITY_ASSET_BLOB): $(ASSET_BLOB) $(ASSET_COMPILER_INPUTS) $(VANILLA_SOURCE_MANIFEST)",
        "$(ENTITY_ASSET_REPORT): $(ENTITY_ASSET_BLOB)",
        "assets: $(ASSET_BLOB) $(ATMOSPHERE_BLOB) $(ATMOSPHERE_REPORT) $(ENTITY_ASSET_BLOB) $(ENTITY_ASSET_REPORT)",
    ] {
        assert!(
            makefile.contains(contract),
            "missing entity asset Makefile contract: {contract}"
        );
    }
    let phony = makefile
        .lines()
        .find(|line| line.starts_with(".PHONY:"))
        .unwrap();
    assert!(phony.split_whitespace().any(|word| word == "entity-assets"));
    assert!(
        !phony
            .split_whitespace()
            .any(|word| { word == "$(ENTITY_ASSET_BLOB)" || word == "$(ENTITY_ASSET_REPORT)" })
    );
}

#[test]
fn make_builds_the_pinned_open_font_for_default_launch() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let makefile = fs::read_to_string(root.join("Makefile"))
        .unwrap()
        .replace("\r\n", "\n");
    let attributes = fs::read_to_string(root.join(".gitattributes"))
        .unwrap()
        .replace("\r\n", "\n");

    assert!(
        attributes
            .lines()
            .any(|line| line == "assets/ui-font-source.json text eol=lf"),
        "the hashed UI-font manifest must retain LF bytes in fresh Windows checkouts"
    );

    for contract in [
        "UI_FONT_SOURCE_MANIFEST ?= assets/ui-font-source.json",
        "UI_FONT_DIR ?= .local/assets/ui-font/389b770410cc0b7c21c85673bfa2077420fe7f65",
        "UI_FONT_SOURCE ?= $(UI_FONT_DIR)/Inter.ttf",
        "FONT_ASSET_BLOB ?= .local/assets/compiled/ui-inter-v1.mcbefont",
        "FONT_ASSET_REPORT ?= .local/assets/compiled/ui-inter-font-assets.json",
        "LOCAL_FONT_ASSET_BLOB ?= .local/assets/compiled/vanilla-v1.mcbefont",
        "LOCAL_FONT_ASSET_REPORT ?= .local/assets/compiled/font-assets.json",
        "FONT_PACK_DIR ?= .local/assets/font-source",
        concat!(
            "FONT_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- outline-font-assets ",
            "--font \"$(UI_FONT_SOURCE)\" --source-manifest \"$(UI_FONT_SOURCE_MANIFEST)\" ",
            "--out \"$(FONT_ASSET_BLOB)\" --report \"$(FONT_ASSET_REPORT)\""
        ),
        concat!(
            "LOCAL_FONT_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- font-assets ",
            "--pack \"$(FONT_PACK_DIR)\" --source-manifest \"$(VANILLA_SOURCE_MANIFEST)\" ",
            "--out \"$(LOCAL_FONT_ASSET_BLOB)\" --report \"$(LOCAL_FONT_ASSET_REPORT)\""
        ),
        "font-assets: $(FONT_ASSET_BLOB) $(FONT_ASSET_REPORT)",
        "font-assets-local:",
        "$(UI_FONT_SOURCE): $(UI_FONT_SOURCE_MANIFEST)",
        "$(FONT_ASSET_BLOB): $(ASSET_COMPILER_INPUTS) $(UI_FONT_SOURCE_MANIFEST) $(UI_FONT_SOURCE)",
        "$(FONT_ASSET_REPORT): $(FONT_ASSET_BLOB)",
        "assets: $(ASSET_BLOB) $(ATMOSPHERE_BLOB) $(ATMOSPHERE_REPORT) $(ENTITY_ASSET_BLOB) $(ENTITY_ASSET_REPORT) $(FONT_ASSET_BLOB) $(FONT_ASSET_REPORT)",
    ] {
        assert!(
            makefile.contains(contract),
            "missing font asset Makefile contract: {contract}"
        );
    }
    let phony = makefile
        .lines()
        .find(|line| line.starts_with(".PHONY:"))
        .unwrap();
    assert!(phony.split_whitespace().any(|word| word == "font-assets"));
    assert!(
        phony
            .split_whitespace()
            .any(|word| word == "font-assets-local")
    );
    let assets = makefile.lines().find(|line| line.starts_with("assets:")).unwrap();
    assert!(assets.contains("FONT_ASSET"));
}

#[test]
fn make_builds_the_pinned_official_hud_carrier_for_default_launch() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let makefile = fs::read_to_string(root.join("Makefile"))
        .unwrap()
        .replace("\r\n", "\n");

    for contract in [
        "HUD_PACK_DIR ?= $(PACK_DIR)",
        "PACK_SENTINEL ?= $(PACK_DIR)/blocks.json",
        "VANILLA_FETCH_INPUTS := scripts/fetch-vanilla-assets.ps1 scripts/fetch-vanilla-assets.sh",
        "vanilla-assets: $(PACK_SENTINEL)",
        "$(PACK_SENTINEL): $(VANILLA_SOURCE_MANIFEST) | $(VANILLA_FETCH_INPUTS)",
        "$(ASSET_BLOB): $(PACK_SENTINEL) $(ASSET_COMPILER_INPUTS)",
        "HUD_ASSET_BLOB ?= .local/assets/compiled/vanilla-v1.mcbehud",
        "HUD_ASSET_REPORT ?= .local/assets/compiled/hud-assets.json",
        "HUD_SOURCE_MANIFEST ?= assets/hud-source-v1001.json",
        concat!(
            "HUD_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- hud-assets ",
            "--pack \"$(HUD_PACK_DIR)\" --source-manifest \"$(HUD_SOURCE_MANIFEST)\" ",
            "--out \"$(HUD_ASSET_BLOB)\" --report \"$(HUD_ASSET_REPORT)\""
        ),
        "hud-assets: $(HUD_ASSET_BLOB) $(HUD_ASSET_REPORT)",
        "$(HUD_ASSET_BLOB): $(ASSET_BLOB) $(ASSET_COMPILER_INPUTS) $(HUD_SOURCE_MANIFEST)",
        "$(HUD_ASSET_REPORT): $(HUD_ASSET_BLOB)",
    ] {
        assert!(
            makefile.contains(contract),
            "missing HUD Makefile contract: {contract}"
        );
    }
    let assets = makefile.lines().find(|line| line.starts_with("assets:")).unwrap();
    assert!(assets.contains("$(HUD_ASSET_BLOB)"));
    assert!(assets.contains("$(HUD_ASSET_REPORT)"));
}

#[test]
fn make_vanilla_pack_sentinel_reacquires_only_when_missing() {
    let make_available = match Command::new("make").arg("--version").output() {
        Ok(output) if output.status.success() => true,
        Ok(_) | Err(_) => false,
    };
    if !make_available {
        eprintln!("skipping executable vanilla-pack Makefile test: `make` is unavailable");
        return;
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let temporary = temporary_directory("make-vanilla-pack-sentinel");
    let pack = temporary.join("resource_pack");
    let sentinel = pack.join("blocks.json");
    let invocations = temporary.join("fetch-invocations.log");
    fs::create_dir_all(&pack).unwrap();
    let producer = format!(
        "echo invocation >> \"{}\" && echo pack > \"{}\"",
        make_path(&invocations),
        make_path(&sentinel)
    );
    let assignments = [
        format!("PACK_DIR={}", make_path(&pack)),
        format!("PACK_SENTINEL={}", make_path(&sentinel)),
        format!("VANILLA_ASSET_FETCH={producer}"),
        "VANILLA_FETCH_INPUTS=".to_owned(),
    ];

    run_make_vanilla_assets(root, &assignments);
    assert_eq!(fs::read_to_string(&invocations).unwrap().lines().count(), 1);
    run_make_vanilla_assets(root, &assignments);
    assert_eq!(fs::read_to_string(&invocations).unwrap().lines().count(), 1);

    fs::remove_file(&sentinel).unwrap();
    run_make_vanilla_assets(root, &assignments);
    assert_eq!(fs::read_to_string(&invocations).unwrap().lines().count(), 2);

    fs::remove_dir_all(temporary).unwrap();
}

#[test]
fn make_client_acquires_compiles_all_assets_then_launches() {
    let make_available = matches!(
        Command::new("make").arg("--version").output(),
        Ok(output) if output.status.success()
    );
    if !make_available {
        eprintln!("skipping executable client dependency-order test: `make` is unavailable");
        return;
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let temporary = temporary_directory("make-client-assets-order");
    let pack = temporary.join("resource_pack");
    let sentinel = pack.join("blocks.json");
    let log = temporary.join("order.log");
    fs::create_dir_all(&pack).unwrap();

    let block = fixture_file(&temporary, "block.bin");
    let light = fixture_file(&temporary, "light.bin");
    let biome = fixture_file(&temporary, "biome.bin");
    let font_source = fixture_file(&temporary, "font.ttf");
    let font_manifest = fixture_file(&temporary, "font-source.json");
    let physics = fixture_file(&temporary, "physics.bin");
    let world = temporary.join("world.mcbea");
    let atmosphere = temporary.join("atmosphere.mcbeatm");
    let atmosphere_report = temporary.join("atmosphere.json");
    let entity = temporary.join("entity.mcbeent");
    let entity_report = temporary.join("entity.json");
    let font = temporary.join("font.mcbefont");
    let font_report = temporary.join("font.json");
    let hud = temporary.join("hud.mcbehud");
    let hud_report = temporary.join("hud.json");

    let assignments = [
        "ASSET_COMPILER_INPUTS=".to_owned(),
        "VANILLA_FETCH_INPUTS=".to_owned(),
        format!("PACK_DIR={}", make_path(&pack)),
        format!("PACK_SENTINEL={}", make_path(&sentinel)),
        format!("BLOCK_REGISTRY={}", make_path(&block)),
        format!("LIGHT_REGISTRY={}", make_path(&light)),
        format!("BIOME_REGISTRY={}", make_path(&biome)),
        format!("ASSET_BLOB={}", make_path(&world)),
        format!("ATMOSPHERE_BLOB={}", make_path(&atmosphere)),
        format!("ATMOSPHERE_REPORT={}", make_path(&atmosphere_report)),
        format!("ENTITY_ASSET_BLOB={}", make_path(&entity)),
        format!("ENTITY_ASSET_REPORT={}", make_path(&entity_report)),
        format!("UI_FONT_SOURCE={}", make_path(&font_source)),
        format!("UI_FONT_SOURCE_MANIFEST={}", make_path(&font_manifest)),
        format!("FONT_ASSET_BLOB={}", make_path(&font)),
        format!("FONT_ASSET_REPORT={}", make_path(&font_report)),
        format!("HUD_ASSET_BLOB={}", make_path(&hud)),
        format!("HUD_ASSET_REPORT={}", make_path(&hud_report)),
        format!("PHYSICS_REGISTRY={}", make_path(&physics)),
        producer_assignment("VANILLA_ASSET_FETCH", "acquire", &log, &[&sentinel]),
        producer_assignment("WORLD_ASSET_COMPILE", "world", &log, &[&world]),
        producer_assignment(
            "ATMOSPHERE_COMPILE",
            "atmosphere",
            &log,
            &[&atmosphere, &atmosphere_report],
        ),
        producer_assignment(
            "ENTITY_ASSET_COMPILE",
            "entity",
            &log,
            &[&entity, &entity_report],
        ),
        producer_assignment("FONT_ASSET_COMPILE", "font", &log, &[&font, &font_report]),
        producer_assignment("HUD_ASSET_COMPILE", "hud", &log, &[&hud, &hud_report]),
        format!(
            "PHYSICS_REGISTRY_COMPILE=echo generated > \"{}\"",
            make_path(&physics)
        ),
        format!("PHYSICS_REGISTRY_CHECK=echo physics >> \"{}\"", make_path(&log)),
        format!("CLIENT_RUN=echo launch >> \"{}\"", make_path(&log)),
    ];

    let output = Command::new("make")
        .current_dir(root)
        .args(["-f", "Makefile", "client"])
        .args(assignments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "make client failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(&log).unwrap().lines().collect::<Vec<_>>(),
        ["acquire", "world", "atmosphere", "entity", "font", "hud", "physics", "launch"]
    );

    fs::remove_dir_all(temporary).unwrap();
}

#[test]
fn make_atmosphere_target_serializes_one_producer_for_missing_and_stale_pairs() {
    let make_available = match Command::new("make").arg("--version").output() {
        Ok(output) if output.status.success() => true,
        Ok(output) => {
            eprintln!(
                "skipping executable Makefile test: `make --version` failed with {}",
                output.status
            );
            false
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("skipping executable Makefile test: `make` is unavailable");
            false
        }
        Err(error) => panic!("failed to probe make: {error}"),
    };
    if !make_available {
        return;
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let temporary = temporary_directory("make-atmosphere-behavior");
    let world = temporary.join("world.mcbea");
    let block = temporary.join("block.bin");
    let light = temporary.join("light.bin");
    let biome = temporary.join("biome.bin");
    let manifest = temporary.join("vanilla-source.json");
    let atmosphere = temporary.join("atmosphere.mcbeatm");
    let report = temporary.join("atmosphere.json");
    let invocations = temporary.join("invocations.log");
    // `$(ASSET_BLOB)` still depends on `$(PACK_SENTINEL)`, so leaving the pack
    // at its default keeps this dependency-ordering test reaching the real
    // Mojang fetch on any checkout without a populated `.local` cache. Pin the
    // pack and stub every upstream producer so the test stays hermetic.
    let pack = temporary.join("resource_pack");
    let sentinel = pack.join("blocks.json");
    let upstream = temporary.join("upstream.log");
    fs::create_dir_all(&pack).unwrap();
    fs::write(&sentinel, b"{}").unwrap();
    for prerequisite in [&block, &light, &biome] {
        fs::write(prerequisite, b"registry").unwrap();
    }
    fs::write(&world, b"world").unwrap();
    fs::copy(root.join("assets/vanilla-source.json"), &manifest).unwrap();
    let baseline = SystemTime::now();
    for prerequisite in [&block, &light, &biome, &manifest] {
        fs::File::options()
            .write(true)
            .open(prerequisite)
            .unwrap()
            .set_modified(baseline - Duration::from_secs(120))
            .unwrap();
    }
    // The stale-manifest scenario below dates the manifest into the future, so
    // the pack sentinel has to stay newer than every manifest timestamp or make
    // would reacquire the pack, and the world blob newer than the sentinel or
    // make would recompile it. Either would leave the atmosphere ordering under
    // test dependent on an upstream producer.
    fs::File::options()
        .write(true)
        .open(&sentinel)
        .unwrap()
        .set_modified(baseline + Duration::from_secs(120))
        .unwrap();
    fs::File::options()
        .write(true)
        .open(&world)
        .unwrap()
        .set_modified(baseline + Duration::from_secs(180))
        .unwrap();

    let producer_script = temporary.join(if cfg!(windows) {
        "produce-atmosphere.ps1"
    } else {
        "produce-atmosphere.sh"
    });
    let producer = if cfg!(windows) {
        fs::write(
            &producer_script,
            format!(
                "Add-Content -LiteralPath '{}' -Value invocation\nSet-Content -LiteralPath '{}' -Value blob\nSet-Content -LiteralPath '{}' -Value report\n",
                invocations.display(), atmosphere.display(), report.display()
            ),
        )
        .unwrap();
        format!(
            "powershell -NoProfile -ExecutionPolicy Bypass -File \"{}\"",
            make_path(&producer_script)
        )
    } else {
        fs::write(
            &producer_script,
            format!(
                "#!/usr/bin/env bash\nset -euo pipefail\necho invocation >> '{}'\necho blob > '{}'\necho report > '{}'\n",
                invocations.display(), atmosphere.display(), report.display()
            ),
        )
        .unwrap();
        format!("bash \"{}\"", make_path(&producer_script))
    };
    let assignments = [
        "ASSET_COMPILER_INPUTS=".to_owned(),
        "VANILLA_FETCH_INPUTS=".to_owned(),
        format!("PACK_DIR={}", make_path(&pack)),
        format!("PACK_SENTINEL={}", make_path(&sentinel)),
        format!("ASSET_BLOB={}", make_path(&world)),
        format!("BLOCK_REGISTRY={}", make_path(&block)),
        format!("LIGHT_REGISTRY={}", make_path(&light)),
        format!("BIOME_REGISTRY={}", make_path(&biome)),
        format!("VANILLA_SOURCE_MANIFEST={}", make_path(&manifest)),
        format!("ATMOSPHERE_BLOB={}", make_path(&atmosphere)),
        format!("ATMOSPHERE_REPORT={}", make_path(&report)),
        format!("ATMOSPHERE_COMPILE={producer}"),
        format!(
            "VANILLA_ASSET_FETCH=echo fetch >> \"{}\"",
            make_path(&upstream)
        ),
        format!(
            "WORLD_ASSET_COMPILE=echo world >> \"{}\"",
            make_path(&upstream)
        ),
    ];

    run_make_atmosphere(root, &assignments);
    assert_eq!(fs::read_to_string(&invocations).unwrap().lines().count(), 1);
    assert!(atmosphere.is_file() && report.is_file());

    fs::remove_file(&report).unwrap();
    run_make_atmosphere(root, &assignments);
    assert_eq!(fs::read_to_string(&invocations).unwrap().lines().count(), 2);
    assert!(atmosphere.is_file() && report.is_file());

    fs::File::options()
        .write(true)
        .open(&manifest)
        .unwrap()
        .set_modified(baseline + Duration::from_secs(60))
        .unwrap();
    run_make_atmosphere(root, &assignments);
    assert_eq!(fs::read_to_string(&invocations).unwrap().lines().count(), 3);
    assert!(atmosphere.is_file() && report.is_file());

    let clouds_override = temporary.join("clouds.png");
    fs::write(&clouds_override, b"synthetic override prerequisite").unwrap();
    let mut override_assignments = assignments.to_vec();
    override_assignments.push(format!(
        "CINNABAR_CLOUDS_PNG={}",
        make_path(&clouds_override)
    ));
    run_make_atmosphere(root, &override_assignments);
    assert_eq!(fs::read_to_string(&invocations).unwrap().lines().count(), 4);

    let mut default_assignments = assignments.to_vec();
    default_assignments.push("CINNABAR_CLOUDS_PNG=".to_owned());
    run_make_atmosphere(root, &default_assignments);
    assert_eq!(fs::read_to_string(&invocations).unwrap().lines().count(), 5);

    assert!(
        !upstream.exists(),
        "atmosphere ordering must never reach the vanilla fetch or world compile: {}",
        fs::read_to_string(&upstream).unwrap_or_default()
    );

    fs::remove_dir_all(temporary).unwrap();
}

#[test]
fn make_report_fallback_recovers_missing_and_stale_reports_with_quoted_arguments() {
    let make_available = Command::new("make")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success());
    if !make_available {
        eprintln!("skipping executable report-recovery test: `make` is unavailable");
        return;
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let temporary = temporary_directory("make-report-fallback");
    let pack = temporary.join("resource_pack");
    let sentinel = pack.join("blocks.json");
    let manifest = temporary.join("vanilla-source.json");
    let block = temporary.join("block.bin");
    let light = temporary.join("light.bin");
    let biome = temporary.join("biome.bin");
    let world = temporary.join("world.mcbea");
    let carrier = temporary.join("entities.mcbeent");
    let report = temporary.join("entities.json");
    let invocations = temporary.join("invocations.log");
    let upstream = temporary.join("upstream.log");
    fs::create_dir_all(&pack).unwrap();
    fs::write(&sentinel, b"{}").unwrap();
    fs::copy(root.join("assets/vanilla-source.json"), &manifest).unwrap();
    for (path, contents) in [
        (&block, b"block".as_slice()),
        (&light, b"light".as_slice()),
        (&biome, b"biome".as_slice()),
        (&world, b"world".as_slice()),
        (&carrier, b"carrier".as_slice()),
    ] {
        fs::write(path, contents).unwrap();
    }

    let baseline = SystemTime::now();
    for path in [&manifest, &block, &light, &biome] {
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(baseline - Duration::from_secs(180))
            .unwrap();
    }
    for (path, offset) in [(&sentinel, 120), (&world, 60), (&carrier, 0)] {
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(baseline - Duration::from_secs(offset))
            .unwrap();
    }

    let producer_script = temporary.join(if cfg!(windows) {
        "produce-entity.ps1"
    } else {
        "produce-entity.sh"
    });
    let producer = if cfg!(windows) {
        fs::write(
            &producer_script,
            format!(
                "param([string]$Label, [string]$First, [string]$Second)\nif ($Label -cne 'quoted value' -or $First -cne 'first value' -or $Second -cne 'second value') {{ throw 'compiler arguments changed' }}\nAdd-Content -LiteralPath '{}' -Value \"$Label|$First|$Second\"\nSet-Content -LiteralPath '{}' -Value carrier\nSet-Content -LiteralPath '{}' -Value report\n",
                invocations.display(), carrier.display(), report.display()
            ),
        )
        .unwrap();
        format!(
            "powershell -NoProfile -ExecutionPolicy Bypass -File \"{}\" \"quoted value\" \"first value\" \"second value\"",
            make_path(&producer_script)
        )
    } else {
        fs::write(
            &producer_script,
            format!(
                "#!/usr/bin/env bash\nset -euo pipefail\n[[ $# == 3 && $1 == 'quoted value' && $2 == 'first value' && $3 == 'second value' ]]\nprintf '%s|%s|%s\\n' \"$1\" \"$2\" \"$3\" >> '{}'\nprintf carrier > '{}'\nprintf report > '{}'\n",
                invocations.display(), carrier.display(), report.display()
            ),
        )
        .unwrap();
        format!(
            "bash \"{}\" \"quoted value\" \"first value\" \"second value\"",
            make_path(&producer_script)
        )
    };
    let assignments = [
        "ASSET_COMPILER_INPUTS=".to_owned(),
        "VANILLA_FETCH_INPUTS=".to_owned(),
        format!("PACK_DIR={}", make_path(&pack)),
        format!("PACK_SENTINEL={}", make_path(&sentinel)),
        format!("VANILLA_SOURCE_MANIFEST={}", make_path(&manifest)),
        format!("BLOCK_REGISTRY={}", make_path(&block)),
        format!("LIGHT_REGISTRY={}", make_path(&light)),
        format!("BIOME_REGISTRY={}", make_path(&biome)),
        format!("ASSET_BLOB={}", make_path(&world)),
        format!("ENTITY_ASSET_BLOB={}", make_path(&carrier)),
        format!("ENTITY_ASSET_REPORT={}", make_path(&report)),
        format!("ENTITY_ASSET_COMPILE={producer}"),
        format!("VANILLA_ASSET_FETCH=echo fetch >> \"{}\"", make_path(&upstream)),
        format!("WORLD_ASSET_COMPILE=echo world >> \"{}\"", make_path(&upstream)),
    ];

    run_make_entity(root, &assignments);
    assert_eq!(
        fs::read_to_string(&invocations)
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        ["quoted value|first value|second value"]
    );
    assert!(carrier.is_file() && report.is_file());

    fs::File::options()
        .write(true)
        .open(&report)
        .unwrap()
        .set_modified(baseline - Duration::from_secs(300))
        .unwrap();
    run_make_entity(root, &assignments);
    assert_eq!(fs::read_to_string(&invocations).unwrap().lines().count(), 2);
    assert!(
        !upstream.exists(),
        "report fallback reached an upstream producer: {}",
        fs::read_to_string(&upstream).unwrap_or_default()
    );

    fs::remove_dir_all(temporary).unwrap();
}

fn run_make_atmosphere(root: &Path, assignments: &[String]) {
    let output = Command::new("make")
        .current_dir(root)
        .args(["-f", "Makefile", "-j4", "atmosphere-assets"])
        .args(assignments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "make atmosphere-assets failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_make_entity(root: &Path, assignments: &[String]) {
    let output = Command::new("make")
        .current_dir(root)
        .args(["-f", "Makefile", "-j4", "entity-assets"])
        .args(assignments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "make entity-assets failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_make_vanilla_assets(root: &Path, assignments: &[String]) {
    let output = Command::new("make")
        .current_dir(root)
        .args(["-f", "Makefile", "vanilla-assets"])
        .args(assignments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "make vanilla-assets failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn make_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn fixture_file(directory: &Path, name: &str) -> PathBuf {
    let path = directory.join(name);
    fs::write(&path, b"fixture").unwrap();
    path
}

fn producer_assignment(variable: &str, label: &str, log: &Path, outputs: &[&Path]) -> String {
    let mut command = format!("{variable}=echo {label} >> \"{}\"", make_path(log));
    for output in outputs {
        command.push_str(&format!(" && echo generated > \"{}\"", make_path(output)));
    }
    command
}
