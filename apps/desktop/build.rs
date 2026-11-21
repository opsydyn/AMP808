use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

fn main() {
    println!("cargo:rustc-check-cfg=cfg(amp808_music_kit_bridge)");
    println!("cargo:rerun-if-changed=macos/music-kit-bridge/Amp808MusicKitBridge.swift");
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }

    let Some(swiftc) = find_swiftc() else {
        println!("cargo:warning=AMP808 MusicKit bridge unavailable: xcrun could not locate swiftc");
        return;
    };
    if !swift_supports_c_exports(&swiftc) {
        println!(
            "cargo:warning=AMP808 MusicKit bridge unavailable: Swift 6.3 or newer is required"
        );
        return;
    }
    let Some(sdk) = macos_sdk() else {
        println!(
            "cargo:warning=AMP808 MusicKit bridge unavailable: xcrun could not locate the macOS SDK"
        );
        return;
    };

    let manifest_dir = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("Cargo provides CARGO_MANIFEST_DIR to build.rs"),
    );
    let out_dir =
        PathBuf::from(env::var_os("OUT_DIR").expect("Cargo provides OUT_DIR to build.rs"));
    let source = manifest_dir.join("macos/music-kit-bridge/Amp808MusicKitBridge.swift");
    let object = out_dir.join("Amp808MusicKitBridge.o");
    let archive = out_dir.join("libAmp808MusicKitBridge.a");
    let header = out_dir.join("Amp808MusicKitBridge.h");
    let target = swift_target();

    let status = Command::new(swiftc)
        .args([
            "-swift-version",
            "6",
            "-parse-as-library",
            "-warnings-as-errors",
        ])
        .arg("-sdk")
        .arg(&sdk)
        .arg("-target")
        .arg(&target)
        .args(["-module-name", "Amp808MusicKitBridge", "-emit-object"])
        .arg("-emit-clang-header-path")
        .arg(&header)
        .arg(&source)
        .arg("-o")
        .arg(&object)
        .status()
        .expect("failed to launch swiftc for the AMP808 MusicKit bridge");
    assert!(
        status.success(),
        "swiftc failed to compile the AMP808 MusicKit bridge with status {status}"
    );

    let status = Command::new("xcrun")
        .args(["libtool", "-static", "-o"])
        .arg(&archive)
        .arg(&object)
        .status()
        .expect("failed to launch libtool for the AMP808 MusicKit bridge");
    assert!(
        status.success(),
        "libtool failed to archive the AMP808 MusicKit bridge with status {status}"
    );

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-search=native=/usr/lib/swift");
    println!("cargo:rustc-link-lib=static=Amp808MusicKitBridge");
    println!("cargo:rustc-link-lib=framework=MusicKit");
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    println!("cargo:rustc-cfg=amp808_music_kit_bridge");
}

fn macos_sdk() -> Option<PathBuf> {
    let output = Command::new("xcrun")
        .args(["--sdk", "macosx", "--show-sdk-path"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout).ok()?;
    let path = path.trim();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

fn swift_supports_c_exports(swiftc: &Path) -> bool {
    let Ok(output) = Command::new(swiftc).arg("--version").output() else {
        return false;
    };
    let Ok(version) = String::from_utf8(output.stdout) else {
        return false;
    };
    let Some(version) = version.split("Swift version ").nth(1) else {
        return false;
    };
    let mut components = version.split('.');
    let Some(major) = components
        .next()
        .and_then(|value| value.parse::<u32>().ok())
    else {
        return false;
    };
    let Some(minor) = components
        .next()
        .and_then(|value| value.parse::<u32>().ok())
    else {
        return false;
    };
    (major, minor) >= (6, 3)
}

fn find_swiftc() -> Option<PathBuf> {
    let output = Command::new("xcrun")
        .args(["--find", "swiftc"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout).ok()?;
    let path = path.trim();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

fn swift_target() -> String {
    let cargo_target = env::var("TARGET").expect("Cargo provides TARGET to build.rs");
    let architecture = if cargo_target.starts_with("aarch64-") {
        "arm64"
    } else if cargo_target.starts_with("x86_64-") {
        "x86_64"
    } else {
        panic!("unsupported macOS architecture for MusicKit bridge: {cargo_target}");
    };
    let deployment_target = env::var("MACOSX_DEPLOYMENT_TARGET").unwrap_or_else(|_| "13.0".into());
    format!("{architecture}-apple-macosx{deployment_target}")
}
