// Copyright 2023 Pants project contributors.
// Licensed under the Apache License, Version 2.0 (see LICENSE).

use std::cell::Cell;
use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use log::info;
use once_cell::sync::OnceCell;
use sha2::{Digest, Sha256};
use termcolor::WriteColor;
use url::Url;

use crate::utils::exe::{binary_full_name, execute, prepare_exe};
use crate::utils::fs::{copy, ensure_directory, path_as_str, rename};
use crate::utils::os::PATHSEP;
use crate::{build_step, BINARY, SCIENCE_TAG};

const BOOTSTRAP_PTEX_TAG: &str = "v0.7.0";

const CARGO: &str = env!("CARGO");
const CARGO_MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");

// N.B.: OUT_DIR and TARGET are not normally available under compilation, but our custom build
// script forwards them.
const OUT_DIR: &str = env!("OUT_DIR");
const TARGET: &str = env!("TARGET");

pub(crate) fn fingerprint(path: &Path) -> Result<String> {
    let mut reader = std::fs::File::open(path)
        .with_context(|| format!("Failed to open {path} for hashing.", path = path.display()))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut reader, &mut hasher).context("Failed to digest stream")?;
    Ok(format!("{digest:x}", digest = hasher.finalize()))
}

pub(crate) fn check_sha256(path: &Path) -> Result<()> {
    let sha256_file = PathBuf::from(format!("{path}.sha256", path = path.display()));
    let contents = std::fs::read_to_string(&sha256_file).with_context(|| {
        format!(
            "Failed to read {sha256_file}",
            sha256_file = sha256_file.display()
        )
    })?;
    let expected_sha256 = contents.split(' ').next().with_context(|| {
        format!(
            "Expected {sha256_file} to have a leading hash",
            sha256_file = sha256_file.display()
        )
    })?;
    assert_eq!(expected_sha256, fingerprint(path)?.as_str());
    Ok(())
}

fn fetch_and_check_trusted_sha256(ptex: &Path, url: &str, dest_dir: &Path) -> Result<()> {
    execute(Command::new(ptex).args(["-O", url]).current_dir(dest_dir))?;

    let sha256_url = format!("{url}.sha256");
    execute(
        Command::new(ptex)
            .args(["-O", &sha256_url])
            .current_dir(dest_dir),
    )?;

    let parsed_url = Url::parse(url).with_context(|| format!("Failed to parse {url}"))?;
    let url_path = PathBuf::from(parsed_url.path());
    let file_name = url_path
        .file_name()
        .with_context(|| format!("Failed to determine file name from {url}"))?;
    info!("Checking downloaded {url} has sha256 reported in {sha256_url}");
    check_sha256(&dest_dir.join(file_name))
}

pub(crate) struct BuildContext {
    pub(crate) workspace_root: PathBuf,
    pub(crate) package_crate_root: PathBuf,
    pub(crate) cargo_output_root: PathBuf,
    target: String,
    target_prepared: Cell<bool>,
    science_repo: Option<PathBuf>,
    cargo_output_bin_dir: PathBuf,
}

impl BuildContext {
    pub(crate) fn new(target: Option<&str>, science_repo: Option<&Path>) -> Result<Self> {
        let target = target.unwrap_or(TARGET).to_string();
        let package_crate_root = PathBuf::from(CARGO_MANIFEST_DIR);
        let workspace_root = package_crate_root
            .join("..")
            .canonicalize()
            .context("Failed to canonicalize workspace root")?;

        let output_root = PathBuf::from(OUT_DIR).join("dist");
        let output_bin_dir = output_root.join("bin");
        Ok(Self {
            workspace_root,
            package_crate_root,
            cargo_output_root: output_root,
            target,
            target_prepared: Cell::new(false),
            science_repo: science_repo.map(Path::to_path_buf),
            cargo_output_bin_dir: output_bin_dir,
        })
    }

    fn ensure_target(&self) -> Result<()> {
        if !self.target_prepared.get() {
            build_step!(
                "Ensuring --target {target} is available",
                target = self.target
            );
            execute(Command::new("rustup").args(["target", "add", &self.target]))?;
            self.target_prepared.set(true);
        }
        Ok(())
    }

    pub(crate) fn obtain_science(&self, dest_dir: &Path) -> Result<PathBuf> {
        if let Some(ref science_from) = self.science_repo {
            self.ensure_target()?;
            build_step!(
                "Building the `science` binary from the source at {science_from}",
                science_from = science_from.display()
            );
            execute(
                Command::new("nox")
                    .args(["-e", "package"])
                    .env(
                        "SCIENCE_LIFT_BUILD_DEST_DIR",
                        format!("{dest_dir}", dest_dir = dest_dir.display()),
                    )
                    .current_dir(science_from),
            )?;
        } else {
            fetch_a_scie_project(self, "lift", SCIENCE_TAG, "science", dest_dir)?;
        }
        let science_exe_path = dest_dir.join(binary_full_name("science"));
        prepare_exe(&science_exe_path)?;
        let science_exe = dest_dir.join("science");
        rename(&science_exe_path, &science_exe)?;
        Ok(science_exe)
    }

    pub(crate) fn build_scie_pants(&self) -> Result<PathBuf> {
        build_step!("Building the scie-pants Rust binary.");
        execute(
            Command::new(CARGO)
                .args([
                    "install",
                    "--path",
                    path_as_str(&self.workspace_root)?,
                    "--target",
                    &self.target,
                    "--root",
                    path_as_str(&self.cargo_output_root)?,
                ])
                // N.B.: This just suppresses a warning about adding this bin dir to your PATH.
                .env(
                    "PATH",
                    [self.cargo_output_bin_dir.to_str().unwrap(), env!("PATH")].join(PATHSEP),
                ),
        )?;
        Ok(self
            .cargo_output_bin_dir
            .join(BINARY)
            .with_extension(env::consts::EXE_EXTENSION))
    }
}

fn fetch_a_scie_project(
    build_context: &BuildContext,
    project_name: &str,
    tag: &str,
    binary_name: &str,
    dest_dir: &Path,
) -> Result<()> {
    static BOOTSTRAP_PTEX: OnceCell<PathBuf> = OnceCell::new();

    let file_name = binary_full_name(binary_name);
    let cache_dir = crate::utils::fs::dev_cache_dir()?
        .join("downloads")
        .join(project_name);
    ensure_directory(&cache_dir, false)?;

    // We proceed with single-checked locking, contention is not a concern in this build process!
    // We only care about correctness.
    let target_dir = cache_dir.join(tag);
    let lock_file = cache_dir.join(format!("{tag}.lck"));
    let lock_fd = std::fs::File::create(&lock_file).with_context(|| {
        format!(
            "Failed to open {path} for locking",
            path = lock_file.display()
        )
    })?;
    let mut lock = fd_lock::RwLock::new(lock_fd);
    let _write_lock = lock.write();
    if !target_dir.exists() {
        let bootstrap_ptex = BOOTSTRAP_PTEX.get_or_try_init::<_, anyhow::Error>(|| {
            build_step!("Bootstrapping a `ptex` binary");
            execute(
                Command::new(CARGO)
                    .args([
                        "install",
                        "--git",
                        "https://github.com/a-scie/ptex",
                        "--tag",
                        BOOTSTRAP_PTEX_TAG,
                        "--root",
                        path_as_str(&build_context.cargo_output_root)?,
                        "--target",
                        &build_context.target,
                        "ptex",
                    ])
                    // N.B.: This just suppresses a warning about adding this bin dir to your PATH.
                    .env(
                        "PATH",
                        [
                            build_context.cargo_output_bin_dir.to_str().unwrap(),
                            env!("PATH"),
                        ]
                        .join(PATHSEP),
                    ),
            )?;
            Ok(build_context.cargo_output_bin_dir.join("ptex"))
        })?;

        build_step!(format!("Fetching the `{project_name}` {tag} binary"));
        let work_dir = cache_dir.join(format!("{tag}.work"));
        ensure_directory(&work_dir, true)?;
        fetch_and_check_trusted_sha256(
            bootstrap_ptex,
            format!(
                "https://github.com/a-scie/{project_name}/releases/download/{tag}/{file_name}",
            )
                .as_str(),
            &work_dir,
        )?;
        rename(&work_dir, &target_dir)?;
    } else {
        build_step!(format!(
            "Loading the `{project_name}` {tag} binary from the cache"
        ));
    }
    copy(&target_dir.join(&file_name), &dest_dir.join(file_name))
}

pub(crate) struct Science(PathBuf);

impl Science {
    pub(crate) fn command(&self) -> Command {
        Command::new(&self.0)
    }
}

pub(crate) fn fetch_science(build_context: &BuildContext) -> Result<Science> {
    let dest_dir = build_context.cargo_output_root.join("science");
    ensure_directory(&dest_dir, true)?;
    let science_exe = build_context.obtain_science(&dest_dir)?;
    Ok(Science(science_exe))
}
