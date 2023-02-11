// Copyright 2023 Pants project contributors.
// Licensed under the Apache License, Version 2.0 (see LICENSE).

use std::cell::Cell;
use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use log::info;
use once_cell::sync::OnceCell;
use proc_exit::{Code, Exit, ExitResult};
use sha2::{Digest, Sha256};
use termcolor::WriteColor;
use url::Url;

use crate::utils::exe::{binary_full_name, execute, prepare_exe};
use crate::utils::fs::{copy, ensure_directory, path_as_str, rename};
use crate::utils::os::PATHSEP;
use crate::{build_step, BINARY, PTEX_TAG, SCIE_JUMP_TAG};

const CARGO: &str = env!("CARGO");
const CARGO_MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");

// N.B.: OUT_DIR and TARGET are not normally available under compilation, but our custom build
// script forwards them.
const OUT_DIR: &str = env!("OUT_DIR");
const TARGET: &str = env!("TARGET");

pub(crate) fn fingerprint(path: &Path) -> Result<String, Exit> {
    let mut reader = std::fs::File::open(path).map_err(|e| {
        Code::FAILURE.with_message(format!(
            "Failed to open {path} for hashing: {e}",
            path = path.display()
        ))
    })?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut reader, &mut hasher)
        .map_err(|e| Code::FAILURE.with_message(format!("Failed to digest stream: {e}")))?;
    Ok(format!("{digest:x}", digest = hasher.finalize()))
}

fn fetch_and_check_trusted_sha256(ptex: &Path, url: &str, dest_dir: &Path) -> ExitResult {
    execute(Command::new(ptex).args(["-O", url]).current_dir(dest_dir))?;

    let sha256_url = format!("{url}.sha256");
    execute(
        Command::new(ptex)
            .args(["-O", &sha256_url])
            .current_dir(dest_dir),
    )?;

    let parsed_url = Url::parse(url)
        .map_err(|e| Code::FAILURE.with_message(format!("Failed to parse {url}: {e}")))?;
    let url_path = PathBuf::from(parsed_url.path());
    let file_name = url_path.file_name().ok_or_else(|| {
        Code::FAILURE.with_message(format!("Failed to determine file name from {url}"))
    })?;
    info!("Checking downloaded {url} has sha256 reported in {sha256_url}");
    crate::check_sha256(&dest_dir.join(file_name))
}

pub(crate) struct BuildContext {
    pub(crate) workspace_root: PathBuf,
    pub(crate) package_crate_root: PathBuf,
    pub(crate) cargo_output_root: PathBuf,
    target: String,
    target_prepared: Cell<bool>,
    ptex_repo: Option<PathBuf>,
    scie_jump_repo: Option<PathBuf>,
    cargo_output_bin_dir: PathBuf,
}

impl BuildContext {
    pub(crate) fn new(
        target: Option<&str>,
        ptex_repo: Option<&Path>,
        scie_jump_repo: Option<&Path>,
    ) -> Result<Self, Exit> {
        let target = target.unwrap_or(TARGET).to_string();
        let package_crate_root = PathBuf::from(CARGO_MANIFEST_DIR);
        let workspace_root = package_crate_root.join("..").canonicalize().map_err(|e| {
            Code::FAILURE.with_message(format!("Failed to canonicalize workspace root: {e}"))
        })?;

        let output_root = PathBuf::from(OUT_DIR).join("dist");
        let output_bin_dir = output_root.join("bin");
        Ok(Self {
            workspace_root,
            package_crate_root,
            cargo_output_root: output_root,
            target,
            target_prepared: Cell::new(false),
            ptex_repo: ptex_repo.map(Path::to_path_buf),
            scie_jump_repo: scie_jump_repo.map(Path::to_path_buf),
            cargo_output_bin_dir: output_bin_dir,
        })
    }

    fn ensure_target(&self) -> ExitResult {
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

    pub(crate) fn obtain_ptex(&self, dest_dir: &Path) -> Result<PathBuf, Exit> {
        if let Some(ref ptex_from) = self.ptex_repo {
            self.ensure_target()?;
            build_step!(
                "Building the `ptex` binary from the source at {ptex_from}",
                ptex_from = ptex_from.display()
            );
            build_a_scie_project(ptex_from, &self.target, dest_dir)?;
        } else {
            fetch_a_scie_project(self, "ptex", PTEX_TAG, "ptex", dest_dir)?;
        }
        let ptex_exe_path = dest_dir.join(binary_full_name("ptex"));
        prepare_exe(&ptex_exe_path)?;
        let ptex_exe = dest_dir.join("ptex");
        rename(&ptex_exe_path, &ptex_exe)?;
        Ok(ptex_exe)
    }

    pub(crate) fn obtain_scie_jump(&self, dest_dir: &Path) -> Result<PathBuf, Exit> {
        if let Some(ref scie_jump_from) = self.scie_jump_repo {
            self.ensure_target()?;
            build_step!(
                "Building the `scie-jump` binary from the source at {scie_jump_from}",
                scie_jump_from = scie_jump_from.display()
            );
            build_a_scie_project(scie_jump_from, &self.target, dest_dir)?;
        } else {
            fetch_a_scie_project(self, "jump", SCIE_JUMP_TAG, "scie-jump", dest_dir)?;
        }
        let scie_jump_exe_path = dest_dir.join(binary_full_name("scie-jump"));
        prepare_exe(&scie_jump_exe_path)?;
        let scie_jump_exe = dest_dir.join("scie_jump");
        rename(&scie_jump_exe_path, &scie_jump_exe)?;
        Ok(scie_jump_exe)
    }

    pub(crate) fn build_scie_pants(&self) -> Result<PathBuf, Exit> {
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
                    vec![self.cargo_output_bin_dir.to_str().unwrap(), env!("PATH")].join(PATHSEP),
                ),
        )?;
        Ok(self
            .cargo_output_bin_dir
            .join(BINARY)
            .with_extension(env::consts::EXE_EXTENSION))
    }
}

pub(crate) struct SkinnyScieTools {
    pub(crate) ptex: PathBuf,
    pub(crate) scie_jump: PathBuf,
}

fn build_a_scie_project(a_scie_project_repo: &Path, target: &str, dest_dir: &Path) -> ExitResult {
    execute(Command::new(CARGO).args([
        "run",
        "--manifest-path",
        path_as_str(&a_scie_project_repo.join("Cargo.toml"))?,
        "-p",
        "package",
        "--",
        "--target",
        target,
        path_as_str(dest_dir)?,
    ]))
    .map(|_| ())
}

fn fetch_a_scie_project(
    build_context: &BuildContext,
    project_name: &str,
    tag: &str,
    binary_name: &str,
    dest_dir: &Path,
) -> ExitResult {
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
    let lock_fd = std::fs::File::create(&lock_file).map_err(|e| {
        Code::FAILURE.with_message(format!(
            "Failed to open {path} for locking: {e}",
            path = lock_file.display()
        ))
    })?;
    let mut lock = fd_lock::RwLock::new(lock_fd);
    let _write_lock = lock.write();
    if !target_dir.exists() {
        let bootstrap_ptex = BOOTSTRAP_PTEX.get_or_try_init(|| {
            build_step!("Bootstrapping a `ptex` binary");
            execute(
                Command::new(CARGO)
                    .args([
                        "install",
                        "--git",
                        "https://github.com/a-scie/ptex",
                        "--tag",
                        PTEX_TAG,
                        "--root",
                        path_as_str(&build_context.cargo_output_root)?,
                        "--target",
                        &build_context.target,
                        "ptex",
                    ])
                    // N.B.: This just suppresses a warning about adding this bin dir to your PATH.
                    .env(
                        "PATH",
                        vec![
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

pub(crate) fn fetch_skinny_scie_tools(
    build_context: &BuildContext,
) -> Result<SkinnyScieTools, Exit> {
    let skinny_scies = build_context.cargo_output_root.join("skinny-scies");
    ensure_directory(&skinny_scies, true)?;
    let ptex_exe = build_context.obtain_ptex(&skinny_scies)?;
    let scie_jump_exe = build_context.obtain_scie_jump(&skinny_scies)?;
    Ok(SkinnyScieTools {
        ptex: ptex_exe,
        scie_jump: scie_jump_exe,
    })
}
