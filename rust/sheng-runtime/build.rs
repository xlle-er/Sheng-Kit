//! Builds the Apple bridge (step 4a): on macOS, every `apple/*.swift` file is compiled with
//! `swiftc` into a static library linked into this crate. Without Swift sources it does nothing.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

const SWIFT_DIR: &str = "apple";
const LIBRARY: &str = "sheng_apple_bridge";
const MODULE: &str = "ShengAppleBridge";
const DEFAULT_DEPLOYMENT_TARGET: &str = "13.0";
/// System frameworks the bridge imports. Translation (macOS 15+) is only used behind
/// availability checks, so its symbols are weak and the linker loads it weakly.
const FRAMEWORKS: [&str; 6] = [
    "Foundation",
    "AVFoundation",
    "Speech",
    "AppKit",
    "SwiftUI",
    "Translation",
];

fn main() {
    println!("cargo:rerun-if-changed={SWIFT_DIR}");
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let sources = swift_sources(Path::new(SWIFT_DIR));
    if sources.is_empty() {
        return;
    }
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    compile(&sources, &out_dir);
    link(&out_dir);
}

/// Swift files directly under `dir`, sorted for a stable command line.
fn swift_sources(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut sources: Vec<PathBuf> = entries
        .map(|entry| entry.expect("readable apple/ entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "swift"))
        .collect();
    sources.sort();
    for source in &sources {
        println!("cargo:rerun-if-changed={}", source.display());
    }
    sources
}

fn target_triple() -> String {
    let arch = match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("aarch64") => "arm64".to_owned(),
        Ok(other) => other.to_owned(),
        Err(_) => panic!("cargo sets CARGO_CFG_TARGET_ARCH"),
    };
    let version = env::var("MACOSX_DEPLOYMENT_TARGET")
        .unwrap_or_else(|_| DEFAULT_DEPLOYMENT_TARGET.to_owned());
    format!("{arch}-apple-macos{version}")
}

fn compile(sources: &[PathBuf], out_dir: &Path) {
    let optimization = match env::var("PROFILE").as_deref() {
        Ok("release") => "-O",
        _ => "-Onone",
    };
    let status = Command::new("xcrun")
        .args(["--sdk", "macosx", "swiftc", "-emit-library", "-static"])
        .args(["-parse-as-library", "-module-name", MODULE, optimization])
        .args(["-target", &target_triple()])
        .arg("-o")
        .arg(out_dir.join(format!("lib{LIBRARY}.a")))
        .args(sources)
        .status()
        .expect("xcrun swiftc can be started");
    assert!(status.success(), "swiftc failed to build the Apple bridge");
}

fn link(out_dir: &Path) {
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static={LIBRARY}");
    if let Some(sdk) = sdk_path() {
        println!("cargo:rustc-link-search=native={sdk}/usr/lib/swift");
    }
    println!("cargo:rustc-link-search=native=/usr/lib/swift");
    for framework in FRAMEWORKS {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
}

fn sdk_path() -> Option<String> {
    let output = Command::new("xcrun")
        .args(["--sdk", "macosx", "--show-sdk-path"])
        .output()
        .ok()?;
    let path = String::from_utf8(output.stdout).ok()?;
    output.status.success().then(|| path.trim().to_owned())
}
