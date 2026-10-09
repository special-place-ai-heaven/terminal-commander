// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

fn collect(dir: &Path, files: &mut Vec<PathBuf>) {
    println!("cargo:rerun-if-changed={}", dir.display());
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            if !matches!(entry.file_name().to_str(), Some("target" | ".git")) {
                collect(&path, files);
            }
        } else if path.extension().is_some_and(|ext| ext == "rs")
            || path
                .file_name()
                .is_some_and(|name| name == "Cargo.toml" || name == "Cargo.lock")
        {
            files.push(path);
        }
    }
}

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let workspace = manifest.parent().and_then(Path::parent).unwrap();
    let root = if workspace.join("crates/core/Cargo.toml").is_file() {
        workspace
    } else {
        &manifest
    };
    let mut files = Vec::new();
    if root == workspace {
        collect(&workspace.join("crates"), &mut files);
        files.extend([workspace.join("Cargo.toml"), workspace.join("Cargo.lock")]);
    } else {
        collect(&manifest, &mut files);
    }
    files.sort();
    let mut hash = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d_u128;
    let mut feed = |bytes: &[u8]| {
        for byte in bytes {
            hash =
                (hash ^ u128::from(*byte)).wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_013b);
        }
    };
    for path in files {
        println!("cargo:rerun-if-changed={}", path.display());
        feed(
            path.strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
                .as_bytes(),
        );
        feed(&[0]);
        feed(&fs::read(&path).expect("build identity input must be readable"));
        feed(&[0]);
    }
    let mut compiler_command = Command::new(env::var_os("RUSTC").unwrap());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        compiler_command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let compiler = compiler_command
        .arg("--version")
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .unwrap_or_default();
    let target = env::var("TARGET").unwrap();
    let profile = env::var("PROFILE").unwrap();
    let mut features: Vec<_> = env::vars()
        .filter_map(|(key, _)| key.strip_prefix("CARGO_FEATURE_").map(str::to_owned))
        .collect();
    features.sort();
    let features = features.join(",");
    for value in [&compiler, &target, &profile, &features] {
        feed(value.as_bytes());
        feed(&[0]);
    }
    println!("cargo:rustc-env=TC_SOURCE_FINGERPRINT=fnv1a128:{hash:032x}");
    println!("cargo:rustc-env=TC_BUILD_COMPILER={compiler}");
    println!("cargo:rustc-env=TC_BUILD_TARGET={target}");
    println!("cargo:rustc-env=TC_BUILD_PROFILE={profile}");
    println!("cargo:rustc-env=TC_BUILD_FEATURES={features}");
    println!("cargo:rerun-if-env-changed=TC_BUILD_ID");
}
