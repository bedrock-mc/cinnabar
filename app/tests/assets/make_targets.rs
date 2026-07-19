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
            "$(ASSET_BLOB): $(ASSET_COMPILER_INPUTS) $(BLOCK_REGISTRY) ",
            "$(LIGHT_REGISTRY) $(BIOME_REGISTRY)"
        ),
        "assets: $(ASSET_BLOB)",
        "client: $(ASSET_BLOB)",
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
        concat!(
            "\t@if [ ! -f \"$@\" ] || [ \"$@\" -ot \"$<\" ]; then ",
            "$(ATMOSPHERE_COMPILE); fi"
        ),
        "\t$(ATMOSPHERE_COMPILE)",
        "atmosphere-assets: $(ATMOSPHERE_BLOB) $(ATMOSPHERE_REPORT)",
        "assets: $(ASSET_BLOB) $(ATMOSPHERE_BLOB) $(ATMOSPHERE_REPORT)",
        "client: $(ASSET_BLOB) $(ATMOSPHERE_BLOB) $(ATMOSPHERE_REPORT)",
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
        "client: $(ASSET_BLOB) $(ATMOSPHERE_BLOB) $(ATMOSPHERE_REPORT) $(ENTITY_ASSET_BLOB) $(ENTITY_ASSET_REPORT)",
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
        "client: $(ASSET_BLOB) $(ATMOSPHERE_BLOB) $(ATMOSPHERE_REPORT) $(ENTITY_ASSET_BLOB) $(ENTITY_ASSET_REPORT) $(FONT_ASSET_BLOB) $(FONT_ASSET_REPORT)",
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
    for default_target in ["assets:", "client:"] {
        let line = makefile
            .lines()
            .find(|line| line.starts_with(default_target))
            .unwrap();
        assert!(line.contains("FONT_ASSET"));
    }
}

#[test]
fn make_builds_the_pinned_official_hud_carrier_for_default_launch() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let makefile = fs::read_to_string(root.join("Makefile"))
        .unwrap()
        .replace("\r\n", "\n");

    for contract in [
        "HUD_PACK_DIR ?= $(PACK_DIR)",
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
    for default_target in ["assets:", "client:"] {
        let line = makefile
            .lines()
            .find(|line| line.starts_with(default_target))
            .unwrap();
        assert!(line.contains("$(HUD_ASSET_BLOB)"));
        assert!(line.contains("$(HUD_ASSET_REPORT)"));
    }
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
    for prerequisite in [&block, &light, &biome] {
        fs::write(prerequisite, b"registry").unwrap();
    }
    fs::write(&world, b"world").unwrap();
    fs::copy(root.join("assets/vanilla-source.json"), &manifest).unwrap();
    let now = SystemTime::now();
    for prerequisite in [&block, &light, &biome, &manifest] {
        fs::File::options()
            .write(true)
            .open(prerequisite)
            .unwrap()
            .set_modified(now - Duration::from_secs(120))
            .unwrap();
    }
    fs::File::options()
        .write(true)
        .open(&world)
        .unwrap()
        .set_modified(now - Duration::from_secs(60))
        .unwrap();

    let producer = format!(
        "echo invocation >> \"{}\" && echo blob > \"{}\" && echo report > \"{}\"",
        make_path(&invocations),
        make_path(&atmosphere),
        make_path(&report)
    );
    let assignments = [
        "ASSET_COMPILER_INPUTS=".to_owned(),
        format!("ASSET_BLOB={}", make_path(&world)),
        format!("BLOCK_REGISTRY={}", make_path(&block)),
        format!("LIGHT_REGISTRY={}", make_path(&light)),
        format!("BIOME_REGISTRY={}", make_path(&biome)),
        format!("VANILLA_SOURCE_MANIFEST={}", make_path(&manifest)),
        format!("ATMOSPHERE_BLOB={}", make_path(&atmosphere)),
        format!("ATMOSPHERE_REPORT={}", make_path(&report)),
        format!("ATMOSPHERE_COMPILE={producer}"),
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
        .set_modified(SystemTime::now() + Duration::from_secs(60))
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

fn make_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
