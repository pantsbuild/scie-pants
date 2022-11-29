// Copyright 2022 Science project contributors.
// Licensed under the Apache License, Version 2.0 (see LICENSE).

use std::env;
use std::path::PathBuf;

use sha2::{Digest, Sha256};

const SCIE_PANTS_BINARY: &str = "scie-pants";

fn main() -> Result<(), String> {
    let bindep_env_var = format!("CARGO_BIN_FILE_SCIE_PANTS_{SCIE_PANTS_BINARY}");
    let path: PathBuf = std::env::var_os(&bindep_env_var)
        .ok_or_else(|| format!("The {bindep_env_var} environment variable was not set."))?
        .into();

    let dest = std::env::var("OUT_DIR")
        .map(|path| {
            PathBuf::from(path)
                .join(format!(
                    "{SCIE_PANTS_BINARY}-{os}-{arch}",
                    os = env::consts::OS,
                    arch = env::consts::ARCH
                ))
                .with_extension(env::consts::EXE_EXTENSION)
        })
        .map_err(|e| format!("{e}"))?;
    std::fs::copy(path, &dest).map_err(|e| {
        format!(
            "Error copying {SCIE_PANTS_BINARY} build to {dest}: {e}",
            dest = dest.display()
        )
    })?;
    println!("cargo:rustc-env=SCIE_STRAP={}", dest.display());

    let mut reader = std::fs::File::open(&dest).map_err(|e| {
        format!(
            "Failed to open {dest} for hashing: {e}",
            dest = dest.display()
        )
    })?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut reader, &mut hasher).map_err(|e| format!("Failed to digest stream: {e}"))?;
    println!(
        "cargo:rustc-env=SCIE_SHA256={digest:x}",
        digest = hasher.finalize()
    );

    Ok(())
}
