// Copyright 2022 Pants project contributors.
// Licensed under the Apache License, Version 2.0 (see LICENSE).

use std::collections::HashMap;
use std::env;
use std::fmt::{Display, Formatter};
use std::fs::Permissions;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Parser;
use lazy_static::lazy_static;
use proc_exit::{Code, Exit, ExitResult};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const BINARY: &str = "scie-pants";

const PTEX_TAG: &str = "v0.4.0";
const SCIE_JUMP_TAG: &str = "v0.5.0";

const CARGO: &str = env!("CARGO");
const CARGO_MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");

// N.B.: OUT_DIR and TARGET are not normally available under compilation, but our custom build
// script forwards them.
const OUT_DIR: &str = env!("OUT_DIR");
const TARGET: &str = env!("TARGET");

#[cfg(target_family = "windows")]
const PATHSEP: &str = ";";

#[cfg(target_family = "unix")]
const PATHSEP: &str = ":";

lazy_static! {
    static ref OS_ARCH: String = format!(
        "{os}-{arch}",
        os = env::consts::OS,
        arch = env::consts::ARCH
    );
}

#[cfg(target_family = "windows")]
fn executable_permissions() -> Option<Permissions> {
    None
}

#[cfg(target_family = "unix")]
fn executable_permissions() -> Option<Permissions> {
    use std::os::unix::fs::PermissionsExt;
    Some(Permissions::from_mode(0o755))
}

fn prepare_exe(path: &Path) -> ExitResult {
    if let Some(permissions) = executable_permissions() {
        std::fs::set_permissions(path, permissions).map_err(|e| {
            Code::FAILURE.with_message(format!(
                "Failed to mark {path} as executable: {e}",
                path = path.display()
            ))
        })?
    }
    Ok(())
}

fn execute(command: &mut Command) -> ExitResult {
    let mut child = command
        .spawn()
        .map_err(|e| Code::FAILURE.with_message(format!("{e}")))?;
    let exit_status = child.wait().map_err(|e| {
        Code::FAILURE.with_message(format!(
            "Failed to gather exit status of command: {command:?}: {e}"
        ))
    })?;
    if !exit_status.success() {
        return Err(Code::FAILURE.with_message(format!(
            "Command {command:?} failed with exit code: {code:?}",
            code = exit_status.code()
        )));
    }
    Ok(())
}

fn path_as_str(path: &Path) -> Result<&str, Exit> {
    path.to_str().ok_or_else(|| {
        Code::FAILURE.with_message(format!("Failed to convert path {path:?} into a UTF-8 str."))
    })
}

fn rename(src: &Path, dst: &Path) -> ExitResult {
    std::fs::rename(src, dst).map_err(|e| {
        Code::FAILURE.with_message(format!(
            "Failed to rename {src} -> {dst}: {e}",
            src = src.display(),
            dst = dst.display()
        ))
    })
}

fn build_scie_jump(scie_jump_from: &str, target: &str, dest_dir: &Path) -> ExitResult {
    execute(Command::new(CARGO).args([
        "run",
        "--manifest-path",
        path_as_str(&PathBuf::from(scie_jump_from).join("Cargo.toml"))?,
        "-p",
        "package",
        "--",
        "--target",
        target,
        path_as_str(dest_dir)?,
    ]))
}

fn fetch_scie_jump(ptex: &Path, dest_dir: &Path) -> ExitResult {
    execute(
        Command::new(ptex)
            .args([
                "-O",
                format!(
                    "https://github.com/a-scie/jump/releases/download/{SCIE_JUMP_TAG}/{scie_jump}",
                    scie_jump = binary_full_name("scie-jump")
                )
                .as_str(),
            ])
            .current_dir(dest_dir),
    )
}

#[derive(Deserialize)]
struct Package {
    python_exe: HashMap<String, String>,
}

#[derive(Deserialize)]
struct Config {
    package: Package,
}

#[derive(Clone)]
struct SpecifiedPath(PathBuf);

impl SpecifiedPath {
    fn new(path: &str) -> Self {
        Self::from(path.to_string())
    }
}

impl From<String> for SpecifiedPath {
    fn from(path: String) -> Self {
        SpecifiedPath(PathBuf::from(path))
    }
}

impl Deref for SpecifiedPath {
    type Target = PathBuf;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<Path> for SpecifiedPath {
    fn as_ref(&self) -> &Path {
        self.0.as_path()
    }
}

impl Display for SpecifiedPath {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.display())
    }
}

#[derive(Parser)]
#[command(about, version)]
struct Args {
    #[arg(long, help = "Override the default --target for this platform.")]
    target: Option<String>,
    #[arg(
        help = "The destination directory for the ptex binary and checksum file.",
        default_value_t = SpecifiedPath::new("dist")
    )]
    dest_dir: SpecifiedPath,
    #[arg(
        long,
        help = format!(
            "Instead of using the released {SCIE_JUMP_TAG} scie-jump, package the scie-jump from \
            the scie-jump project repo at this directory."
        )
    )]
    scie_jump: Option<String>,
    #[arg(
        long,
        help = "Refresh the tools lock before building the tools.pex",
        default_value_t = false
    )]
    update_lock: bool,
}

fn binary_full_name(name: &str) -> String {
    format!(
        "{name}-{os_arch}{exe}",
        os_arch = *OS_ARCH,
        exe = env::consts::EXE_SUFFIX
    )
}

fn main() -> ExitResult {
    let args = Args::parse();
    let dest_dir = args.dest_dir;
    if dest_dir.is_file() {
        return Err(Code::FAILURE.with_message(format!(
            "The specified dest_dir of {} is a file. Not overwriting",
            dest_dir.display()
        )));
    }

    let target = args.target.unwrap_or_else(|| TARGET.to_string());
    // Just in case this target is not already installed.
    execute(Command::new("rustup").args(["target", "add", &target]))?;

    let package_crate_root = PathBuf::from(CARGO_MANIFEST_DIR);
    let workspace_root = package_crate_root.join("..");
    let output_root = PathBuf::from(OUT_DIR).join("dist");
    let output_bin_dir = output_root.join("bin");
    execute(
        Command::new(CARGO)
            .args([
                "install",
                "--path",
                path_as_str(&workspace_root)?,
                "--target",
                &target,
                "--root",
                path_as_str(&output_root)?,
            ])
            // N.B.: This just suppresses a warning about adding this bin dir to your PATH.
            .env(
                "PATH",
                vec![output_bin_dir.to_str().unwrap(), env!("PATH")].join(PATHSEP),
            ),
    )?;

    let scie_pants_exe = output_bin_dir
        .join(BINARY)
        .with_extension(env::consts::EXE_EXTENSION);
    let mut reader = std::fs::File::open(&scie_pants_exe).map_err(|e| {
        Code::FAILURE.with_message(format!(
            "Failed to open {src} for hashing: {e}",
            src = scie_pants_exe.display()
        ))
    })?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut reader, &mut hasher)
        .map_err(|e| Code::FAILURE.with_message(format!("Failed to digest stream: {e}")))?;
    let scie_pants_digest = hasher.finalize();

    // 1. Get a bootstrap ptex to use to download the rest of the pre-built binary releases:
    execute(
        Command::new(CARGO)
            .args([
                "install",
                "--git",
                "https://github.com/a-scie/ptex",
                "--tag",
                PTEX_TAG,
                "--root",
                path_as_str(&output_root)?,
                "ptex",
            ])
            // N.B.: This just suppresses a warning about adding this bin dir to your PATH.
            .env(
                "PATH",
                vec![output_bin_dir.to_str().unwrap(), env!("PATH")].join(PATHSEP),
            ),
    )?;
    let bootstrap_ptex = PathBuf::from(OUT_DIR).join("bin").join("ptex");

    let pbt_dir = package_crate_root.join("pbt");

    // 2. Fetch the production ptex for the current platform:
    let ptex_exe_full_name = binary_full_name("ptex");
    execute(
        Command::new(&bootstrap_ptex)
            .args([
                "-O",
                format!(
                    "https://github.com/a-scie/ptex/releases/download/{PTEX_TAG}/{ptex}",
                    ptex = ptex_exe_full_name
                )
                .as_str(),
            ])
            .current_dir(&pbt_dir),
    )?;
    let ptex_exe_path = pbt_dir.join(ptex_exe_full_name);
    prepare_exe(&ptex_exe_path)?;
    let ptex_exe = pbt_dir.join("ptex");
    rename(&ptex_exe_path, &ptex_exe)?;

    // 3. Fetch the production scie-jump for the current platform:
    if let Some(scie_jump_from) = args.scie_jump {
        build_scie_jump(&scie_jump_from, &target, &pbt_dir)?;
    } else {
        fetch_scie_jump(&bootstrap_ptex, &pbt_dir)?;
    }

    // 4. Retrieve PYTHON_EXE from our custom lift manifest metadata.
    let lift = pbt_dir.join("lift.json");
    let data = std::fs::read(&lift).map_err(|e| {
        Code::FAILURE.with_message(format!(
            "Failed to read contents of {lift}: {e}",
            lift = lift.display()
        ))
    })?;
    let config: Config = serde_json::from_slice(data.as_slice()).map_err(|e| {
        Code::FAILURE.with_message({
            format!(
                "Failed to parse package config from {lift}: {e}",
                lift = lift.display()
            )
        })
    })?;
    let python_exe = config
        .package
        .python_exe
        .get(OS_ARCH.as_str())
        .ok_or_else(|| {
            Code::FAILURE.with_message(format!(
                "The lift manifest at {lift} is missing a package.python_exe entry for {os_arch}",
                lift = lift.display(),
                os_arch = *OS_ARCH
            ))
        })?;

    // 5. Execute scie-jump to boot-pack the `pbt` binary.
    let scie_jump_exe = pbt_dir.join(binary_full_name("scie-jump"));
    prepare_exe(&scie_jump_exe)?;
    let pbt_env = [
        ("OS", env::consts::OS),
        ("ARCH", env::consts::ARCH),
        ("PYTHON_EXE", python_exe),
    ];
    let pbt_exe = pbt_dir.join("pbt");
    if pbt_exe.exists() {
        std::fs::remove_file(&pbt_exe).map_err(|e| {
            Code::FAILURE.with_message(format!(
                "Failed to remove existing {pbt}: {e}",
                pbt = pbt_exe.display()
            ))
        })?;
    }
    execute(
        Command::new(&scie_jump_exe)
            .envs(pbt_env)
            .current_dir(&pbt_dir),
    )?;

    // 6. Build the scie-pants tools wheel.
    let tools_src_path = workspace_root.join("tools").join("src");
    let tools_src = path_as_str(&tools_src_path)?;
    let find_links_dir = tempfile::tempdir().map_err(|e| {
        Code::FAILURE.with_message(format!(
            "Failed to create temporary find-links directory for the scie-jump wheel: {e}"
        ))
    })?;
    let find_links = path_as_str(find_links_dir.path())?;

    // N.B.: We set SOURCE_DATE_EPOCH so that we get a reproducible wheel build here via the flit
    // build backend we have set up in pyproject.toml.
    // See: https://flit.pypa.io/en/stable/reproducible.html
    //
    // We use the start of MS-DOS time: 1/1/1980 00:00:0.0, which is what zipfiles use (see section
    // 4.4.6 of https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT). The value 315532800 is
    // the number of seconds from the start of UNIX time (1/1/1970 00:00:0.0) until then.
    execute(
        Command::new(&pbt_exe)
            .env("SOURCE_DATE_EPOCH", "315532800")
            .envs(pbt_env)
            .args([
                "pip",
                "wheel",
                "--use-pep517",
                "--no-deps",
                "--wheel-dir",
                find_links,
                tools_src,
            ]),
    )?;

    // 7. Run `pbt pex ...` on the scie-pants wheel to get tools.pex

    let lock_path = tools_src_path.join("lock.json");
    let lock = path_as_str(&lock_path)?;

    if args.update_lock {
        execute(Command::new(&pbt_exe).envs(pbt_env).args([
            "pex3",
            "lock",
            "create",
            "--style",
            "universal",
            "--pip-version",
            "22.3",
            "--resolver-version",
            "pip-2020-resolver",
            "--no-build",
            "--find-links",
            find_links,
            "--path-mapping",
            &format!("FIND_LINKS|{find_links}|The temporary find links directory."),
            "-o",
            lock,
            "--indent",
            "2",
            "scie-pants",
        ]))?;
    }

    let scie_pants_package_dir = package_crate_root.join("scie-pants");
    let tools_pex_path = scie_pants_package_dir.join("tools.pex");
    let tools_pex = path_as_str(&tools_pex_path)?;
    execute(Command::new(&pbt_exe).envs(pbt_env).args([
        "pex",
        "--no-emit-warnings",
        "--lock",
        lock,
        "--find-links",
        find_links,
        "--path-mapping",
        &format!("FIND_LINKS|{find_links}"),
        "-c",
        "conscript",
        "-o",
        tools_pex,
        "--venv",
    ]))?;

    // 8. Setup the scie-pants boot-pack.
    let file_name = binary_full_name(BINARY);
    let scie_base_dst = scie_pants_package_dir.join(&file_name);
    let scie_jump_dst = scie_pants_package_dir.join(binary_full_name("scie-jump"));
    let ptex_dst = scie_pants_package_dir.join(binary_full_name("ptex"));
    rename(&scie_pants_exe, &scie_base_dst)?;
    rename(&scie_jump_exe, &scie_jump_dst)?;
    rename(&ptex_exe, &ptex_dst)?;

    // 9. Run the boot-pack and ...
    let scie_pants_lift =
        scie_pants_package_dir.join(format!("lift.{os_arch}.json", os_arch = *OS_ARCH));
    let src = scie_pants_package_dir.join("scie-pants");
    if src.exists() {
        std::fs::remove_file(&src).map_err(|e| {
            Code::FAILURE.with_message(format!(
                "Failed to remove existing {src}: {e}",
                src = src.display()
            ))
        })?;
    }
    execute(
        Command::new(&scie_jump_dst)
            .arg(&scie_pants_lift)
            .current_dir(&scie_pants_package_dir),
    )?;

    //

    std::fs::create_dir_all(&dest_dir).map_err(|e| {
        Code::FAILURE.with_message(format!(
            "Failed to create dest_dir {dest_dir}: {e}",
            dest_dir = dest_dir.display()
        ))
    })?;

    let dst = dest_dir.join(&file_name);
    std::fs::copy(&src, &dst).map_err(|e| {
        Code::FAILURE.with_message(format!(
            "Failed to copy {src} to {dst}: {e}",
            src = src.display(),
            dst = dst.display()
        ))
    })?;

    let fingerprint_file = dst.with_file_name(format!("{file_name}.sha256"));
    std::fs::write(
        &fingerprint_file,
        format!("{scie_pants_digest:x} *{file_name}\n"),
    )
    .map_err(|e| {
        Code::FAILURE.with_message(format!(
            "Failed to write fingerprint file {fingerprint_file}: {e}",
            fingerprint_file = fingerprint_file.display()
        ))
    })?;

    eprintln!(
        "Wrote the {BINARY} (target: {target}) to {dst}",
        dst = dst.display()
    );

    Ok(())
}
