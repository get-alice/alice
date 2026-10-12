//! `ALICE_BUILD` identifies the daemon API a binary serves: a hash of this crate's sources, so the
//! desktop app and CLI built from one tree agree, and any change yields a new identity.
use std::{fs, path::Path};

fn collect(dir: &Path, files: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, files);
        } else {
            files.push(path);
        }
    }
}

fn main() {
    let mut files = vec![Path::new("Cargo.toml").to_path_buf()];
    for dir in ["src", "locales"] {
        println!("cargo:rerun-if-changed={dir}");
        collect(Path::new(dir), &mut files);
    }
    println!("cargo:rerun-if-changed=Cargo.toml");
    files.sort();
    // FNV-1a: stable across toolchains, unlike std's hasher.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for file in files {
        let path = file.to_string_lossy().replace('\\', "/");
        for byte in path
            .bytes()
            .chain([0])
            .chain(fs::read(&file).unwrap_or_default())
            .chain([0])
        {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    println!("cargo:rustc-env=ALICE_BUILD={hash:016x}");
}
