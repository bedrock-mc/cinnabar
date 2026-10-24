use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=native/platform.c");
    println!("cargo:rerun-if-changed=native/texture.c");
    println!("cargo:rerun-if-changed=native/pad_masks.def");
    println!("cargo:rerun-if-env-changed=VITASDK");
    println!("cargo:rerun-if-env-changed=CINNABAR_VITAGL_SOURCE");
    generate_pad_masks();
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("vita") {
        return;
    }
    let sdk = PathBuf::from(env::var_os("VITASDK").expect("set VITASDK to a hard-float VitaSDK"));
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    let mut native = cc::Build::new();
    if let Some(source) = env::var_os("CINNABAR_VITAGL_SOURCE") {
        let source = PathBuf::from(source);
        let archive = source.join("libvitaGL.a");
        assert!(archive.is_file(), "build the selected VitaGL source first");
        println!("cargo:rerun-if-changed={}", archive.display());
        println!(
            "cargo:rerun-if-changed={}",
            source.join("source/vitaGL.h").display()
        );
        println!("cargo:rustc-link-search=native={}", source.display());
        native.include(source.join("source"));
    }
    native
        .compiler(sdk.join(format!("bin/arm-vita-eabi-gcc{suffix}")))
        .archiver(sdk.join(format!("bin/arm-vita-eabi-ar{suffix}")))
        .include(sdk.join("arm-vita-eabi/include"))
        .file("native/platform.c")
        .file("native/texture.c")
        .flag("-std=c11")
        .flag("-mfloat-abi=hard")
        .flag("-mfpu=neon")
        .warnings(true)
        .warnings_into_errors(true)
        .compile("cinnabar_vita_platform");
    println!(
        "cargo:rustc-link-search=native={}",
        sdk.join("arm-vita-eabi/lib").display()
    );
    for library in [
        "vitaGL",
        "vitashark",
        "SceShaccCgExt",
        "mathneon",
        "stdc++",
        "m",
        "SceGxm_stub",
        "SceDisplay_stub",
        "SceSysmodule_stub",
        "SceShaccCg_stub",
        "SceCommonDialog_stub",
        "SceKernelDmacMgr_stub",
        "SceAppMgr_stub",
        "SceAppUtil_stub",
        "SceCtrl_stub",
        "SceTouch_stub",
        "ScePower_stub",
        "taihen_stub",
    ] {
        println!("cargo:rustc-link-lib=static={library}");
    }
}

/// Generate the Rust side of the SDK-checked C button table, including host tests.
fn generate_pad_masks() {
    let table = fs::read_to_string("native/pad_masks.def").expect("Vita button table");
    let mut bindings = String::new();
    for line in table.lines().filter(|line| line.starts_with("CBN_BUTTON(")) {
        let fields: Vec<_> = line
            .strip_prefix("CBN_BUTTON(")
            .and_then(|line| line.strip_suffix(')'))
            .expect("CBN_BUTTON syntax")
            .split(',')
            .map(str::trim)
            .collect();
        assert_eq!(fields.len(), 3, "button name, SDK enum, and mask");
        let name = fields[0];
        assert!(name.bytes().all(|c| c.is_ascii_uppercase() || c == b'_'));
        let value =
            u32::from_str_radix(fields[2].trim_start_matches("0x"), 16).expect("hex button mask");
        assert!(value.is_power_of_two(), "a button occupies exactly one bit");
        bindings.push_str(&format!("pub const {name}: u32 = {value};\n"));
    }
    assert!(!bindings.is_empty(), "empty Vita button table");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    fs::write(out.join("pad_masks.rs"), bindings).expect("generated Vita buttons");
}
