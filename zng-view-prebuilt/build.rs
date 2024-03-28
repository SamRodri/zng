use std::{env, hash::Hasher, path::Path};

use base64::Engine;
use hashers::jenkins::spooky_hash::SpookyHasher;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let mut lib = Path::new(&manifest_dir).join("lib");

    println!("cargo:rerun-if-changed={}", lib.display());

    #[allow(unused_variables)]
    let file = "";

    #[cfg(target_os = "windows")]
    let file = "zng_view.dll";

    #[cfg(target_os = "linux")]
    let file = "libzng_view.so";

    #[cfg(target_os = "macos")]
    let file = "libzng_view.dylib";

    if file.is_empty() {
        panic!("unsuported OS");
    }

    lib = lib.join(file);

    if lib.exists() {
        println!("cargo:rustc-cfg=zng_lib_embedded");
        println!("cargo:rustc-env=ZNG_VIEW_LIB={}", lib.canonicalize().unwrap().display());

        let lib_bytes = std::fs::read(lib).unwrap();

        // just to identify build.
        let mut hasher = SpookyHasher::new(u64::from_le_bytes(*b"prebuild"), u64::from_le_bytes(*b"view-lib"));
        hasher.write(&lib_bytes);
        let (a, b) = hasher.finish128();
        let mut hash = [0; 16];
        hash[..8].copy_from_slice(&a.to_le_bytes());
        hash[8..].copy_from_slice(&b.to_le_bytes());

        println!(
            "cargo:rustc-env=ZNG_VIEW_LIB_HASH={}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash)
        );
    } else {
        println!("cargo:warning=missing `{file}`, run `do prebuild`");
    }
}