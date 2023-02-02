// Copyright 2022 Pants project contributors.
// Licensed under the Apache License, Version 2.0 (see LICENSE).

use std::cell::Cell;
use std::env;
use std::fmt::{Display, Formatter};
use std::fs::Permissions;
use std::io::Write;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU8, Ordering};

use clap::{arg, command, Parser, Subcommand};
use lazy_static::lazy_static;
use log::{info, warn};
use once_cell::sync::OnceCell;
use proc_exit::{Code, Exit, ExitResult};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};
use url::Url;

const BINARY: &str = "scie-pants";

const PTEX_TAG: &str = "v0.6.0";
const SCIE_JUMP_TAG: &str = "v0.10.0";

const CARGO: &str = env!("CARGO");
const CARGO_MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");

// N.B.: OUT_DIR and TARGET are not normally available under compilation, but our custom build
// script forwards them.
const OUT_DIR: &str = env!("OUT_DIR");
const TARGET: &str = env!("TARGET");

#[cfg(windows)]
const PATHSEP: &str = ";";

#[cfg(windows)]
const EOL: &str = "\r\n";

#[cfg(unix)]
const PATHSEP: &str = ":";

#[cfg(unix)]
const EOL: &str = "\n";

#[derive(Eq, PartialEq)]
enum Platform {
    LinuxAarch64,
    LinuxX86_64,
    MacOSAarch64,
    MacOSX86_64,
    WindowsX86_64,
}

impl Platform {
    fn current() -> Result<Self, Exit> {
        match (env::consts::OS, env::consts::ARCH) {
            ("linux", "aarch64") => Ok(Self::LinuxAarch64),
            ("linux", "x86_64") => Ok(Self::LinuxX86_64),
            ("macos", "aarch64") => Ok(Self::MacOSAarch64),
            ("macos", "x86_64") => Ok(Self::MacOSX86_64),
            ("windows", "x86_64") => Ok(Self::WindowsX86_64),
            _ => Err(Code::FAILURE.with_message({
                format!(
                    "Unsupported platform: os={os} arch={arch}",
                    os = env::consts::OS,
                    arch = env::consts::ARCH
                )
            })),
        }
    }

    fn to_str(&self) -> &str {
        match self {
            Platform::LinuxAarch64 => "linux-aarch64",
            Platform::LinuxX86_64 => "linux-x86_64",
            Platform::MacOSAarch64 => "macos-aarch64",
            Platform::MacOSX86_64 => "macos-x86_64",
            Platform::WindowsX86_64 => "windows-x86_64",
        }
    }
}

impl Display for Platform {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.to_str())
    }
}

lazy_static! {
    static ref CURRENT_PLATFORM: Platform = Platform::current().unwrap();
}

macro_rules! log {
    ($color:expr, $msg:expr $(,)?) => {
        let mut stderr = StandardStream::stderr(ColorChoice::Always);
        stderr
            .set_color(ColorSpec::new().set_fg(Some($color))).unwrap();
        writeln!(&mut stderr, $msg).unwrap();
        stderr.reset().unwrap();
    };
    ($color:expr, $msg:expr, $($arg:tt)*) => {
        let mut stderr = StandardStream::stderr(ColorChoice::Always);
        stderr
            .set_color(ColorSpec::new().set_fg(Some($color))).unwrap();
        writeln!(&mut stderr, "{}", format!($msg, $($arg)*)).unwrap();
        stderr.reset().unwrap();
    };
}

lazy_static! {
    static ref BUILD_STEP: AtomicU8 = AtomicU8::new(1);
}

macro_rules! build_step {
    ($msg:expr $(,)?) => {
        log!(Color::Cyan, "{:>2}.) {}...", BUILD_STEP.fetch_add(1, Ordering::Relaxed), $msg);
    };
    ($msg:expr, $($arg:tt)*) => {
        log!(
            Color::Cyan,
            "{:>2}.) {}...",
            BUILD_STEP.fetch_add(1, Ordering::Relaxed),
            format!($msg, $($arg)*)
        );
    };
}

macro_rules! integration_test {
    ($msg:expr $(,)?) => {
        log!(Color::Magenta, ">> {}", $msg);
    };
    ($msg:expr, $($arg:tt)*) => {
        log!(Color::Magenta, ">> {}", format!($msg, $($arg)*));
    };
}

#[cfg(windows)]
fn executable_permissions() -> Option<Permissions> {
    None
}

#[cfg(unix)]
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

fn execute_with_input(command: &mut Command, stdin_data: &[u8]) -> Result<Output, Exit> {
    _execute_with_input(command, Some(stdin_data))
}

fn execute(command: &mut Command) -> Result<Output, Exit> {
    _execute_with_input(command, None)
}

fn _execute_with_input(command: &mut Command, stdin_data: Option<&[u8]>) -> Result<Output, Exit> {
    info!("Executing {command:#?}");
    if stdin_data.is_some() {
        command.stdin(std::process::Stdio::piped());
    }
    let mut child = command.spawn().map_err(|e| {
        Code::FAILURE.with_message(format!("Failed to spawn command: {command:?}: {e}"))
    })?;
    if let Some(stdin_data) = stdin_data {
        child
            .stdin
            .as_mut()
            .expect("We just set a stdin pipe above")
            .write(stdin_data)
            .map_err(|e| {
                Code::FAILURE.with_message(format!(
                    "Failed to write {stdin_data:?} to sub-process stdin: {e}"
                ))
            })?;
    }
    let output = child.wait_with_output().map_err(|e| {
        Code::FAILURE.with_message(format!(
            "Failed to gather exit status of command: {command:?}: {e}"
        ))
    })?;
    if !output.status.success() {
        let mut message_lines = vec![format!(
            "Command {command:?} failed with exit code: {code:?}",
            code = output.status.code()
        )];
        if output.stdout.is_empty() {
            message_lines.push("STDOUT not captured.".to_string())
        } else {
            message_lines.push("STDOUT:".to_string());
            message_lines.push(String::from_utf8_lossy(output.stdout.as_slice()).to_string());
        }
        if output.stderr.is_empty() {
            message_lines.push("STDERR not captured.".to_string())
        } else {
            message_lines.push("STDERR:".to_string());
            message_lines.push(String::from_utf8_lossy(output.stderr.as_slice()).to_string());
        }
        return Err(Code::FAILURE.with_message(message_lines.join(EOL)));
    }
    Ok(output)
}

fn path_as_str(path: &Path) -> Result<&str, Exit> {
    path.to_str().ok_or_else(|| {
        Code::FAILURE.with_message(format!("Failed to convert path {path:?} into a UTF-8 str."))
    })
}

fn hardlink(src: &Path, dst: &Path) -> ExitResult {
    info!(
        "Hard linking {src} -> {dst}",
        src = src.display(),
        dst = dst.display()
    );
    std::fs::hard_link(src, dst).map_err(|e| {
        Code::FAILURE.with_message(format!(
            "Failed to hard link {src} -> {dst}: {e}",
            src = src.display(),
            dst = dst.display()
        ))
    })
}

fn softlink(src: &Path, dst: &Path) -> ExitResult {
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    #[cfg(windows)]
    use std::os::windows::fs::symlink_file as symlink;

    info!(
        "Soft linking {src} -> {dst}",
        src = src.display(),
        dst = dst.display()
    );
    symlink(src, dst).map_err(|e| {
        Code::FAILURE.with_message(format!(
            "Failed to soft link {src} -> {dst}: {e}",
            src = src.display(),
            dst = dst.display()
        ))
    })
}

fn rename(src: &Path, dst: &Path) -> ExitResult {
    info!(
        "Renaming {src} -> {dst}",
        src = src.display(),
        dst = dst.display()
    );
    std::fs::rename(src, dst).map_err(|e| {
        Code::FAILURE.with_message(format!(
            "Failed to rename {src} -> {dst}: {e}",
            src = src.display(),
            dst = dst.display()
        ))
    })
}

fn copy(src: &Path, dst: &Path) -> ExitResult {
    info!(
        "Copying {src} -> {dst}",
        src = src.display(),
        dst = dst.display()
    );
    std::fs::copy(src, dst)
        .map_err(|e| {
            Code::FAILURE.with_message(format!(
                "Failed to copy {src} -> {dst}: {e}",
                src = src.display(),
                dst = dst.display()
            ))
        })
        .map(|_| ())
}

fn remove_dir(path: &Path) -> ExitResult {
    if path.exists() {
        std::fs::remove_dir_all(path).map_err(|e| {
            Code::FAILURE.with_message(format!(
                "Failed to remove directory at {path}: {e}",
                path = path.display()
            ))
        })
    } else {
        Ok(())
    }
}

fn ensure_directory(path: &Path, clean: bool) -> ExitResult {
    if clean {
        if let Err(e) = remove_dir(path) {
            warn!(
                "Failed to clean directory at {path}: {e}",
                path = path.display()
            )
        }
    }
    std::fs::create_dir_all(path).map_err(|e| {
        Code::FAILURE.with_message(format!(
            "Failed to create directory at {path}: {e}",
            path = path.display()
        ))
    })
}

fn create_tempdir() -> Result<TempDir, Exit> {
    tempfile::tempdir().map_err(|e| {
        Code::FAILURE.with_message(format!("Failed to create a new temporary directory: {e}"))
    })
}

fn touch(path: &Path) -> ExitResult {
    write_file(path, true, [])
}

fn write_file<C: AsRef<[u8]>>(path: &Path, append: bool, content: C) -> ExitResult {
    if let Some(parent) = path.parent() {
        ensure_directory(parent, false)?;
    }
    let mut fd = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .open(path)
        .map_err(|e| {
            Code::FAILURE.with_message(format!("Failed to open {path}: {e}", path = path.display()))
        })?;
    fd.write_all(content.as_ref()).map_err(|e| {
        Code::FAILURE.with_message(format!(
            "Failed to touch {path}: {e}",
            path = path.display()
        ))
    })
}

fn canonicalize(path: &Path) -> Result<PathBuf, Exit> {
    path.canonicalize().map_err(|e| {
        Code::FAILURE.with_message(format!(
            "Failed to canonicalize {path} to an absolute, resolved path: {e}",
            path = path.display()
        ))
    })
}

fn binary_full_name(name: &str) -> String {
    format!(
        "{name}-{platform}{exe}",
        platform = *CURRENT_PLATFORM,
        exe = env::consts::EXE_SUFFIX
    )
}

fn fingerprint(path: &Path) -> Result<String, Exit> {
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

fn check_sha256(path: &Path) -> ExitResult {
    let sha256_file = PathBuf::from(format!("{path}.sha256", path = path.display()));
    let contents = std::fs::read_to_string(&sha256_file).map_err(|e| {
        Code::FAILURE.with_message(format!(
            "Failed to read {sha256_file}: {e}",
            sha256_file = sha256_file.display()
        ))
    })?;
    let expected_sha256 = contents.split(' ').next().ok_or_else(|| {
        Code::FAILURE.with_message(format!(
            "Expected {sha256_file} to have a leading hash",
            sha256_file = sha256_file.display()
        ))
    })?;
    assert_eq!(expected_sha256, fingerprint(path)?.as_str());
    Ok(())
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
    check_sha256(&dest_dir.join(file_name))
}

struct BuildContext {
    target: String,
    target_prepared: Cell<bool>,
    ptex_repo: Option<PathBuf>,
    scie_jump_repo: Option<PathBuf>,
    workspace_root: PathBuf,
    package_crate_root: PathBuf,
    cargo_output_root: PathBuf,
    cargo_output_bin_dir: PathBuf,
}

impl BuildContext {
    fn new(
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
            target,
            target_prepared: Cell::new(false),
            ptex_repo: ptex_repo.map(Path::to_path_buf),
            scie_jump_repo: scie_jump_repo.map(Path::to_path_buf),
            workspace_root,
            package_crate_root,
            cargo_output_root: output_root,
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

    fn obtain_ptex(&self, dest_dir: &Path) -> Result<PathBuf, Exit> {
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

    fn obtain_scie_jump(&self, dest_dir: &Path) -> Result<PathBuf, Exit> {
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
}

struct SkinnyScieTools {
    ptex: PathBuf,
    scie_jump: PathBuf,
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

fn dev_cache_dir() -> Result<PathBuf, Exit> {
    if let Ok(cache_dir) = env::var("SCIE_PANTS_DEV_CACHE") {
        let cache_path = PathBuf::from(cache_dir);
        ensure_directory(&cache_path, false)?;
        return cache_path.canonicalize().map_err(|e| {
            Code::FAILURE.with_message(format!(
                "Failed to resolve the absolute path of SCIE_PANTS_DEV_CACHE={cache_dir}: {e}",
                cache_dir = cache_path.display()
            ))
        });
    }

    let cache_dir = dirs::cache_dir()
        .ok_or_else(|| {
            Code::FAILURE.with_message(
                "Failed to look up user cache dir for caching scie project downloads".to_string(),
            )
        })?
        .join("scie-pants")
        .join("dev");
    ensure_directory(&cache_dir, false)?;
    Ok(cache_dir)
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
    let cache_dir = dev_cache_dir()?.join("downloads").join(project_name);
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

fn fetch_skinny_scie_tools(build_context: &BuildContext) -> Result<SkinnyScieTools, Exit> {
    let skinny_scies = build_context.cargo_output_root.join("skinny-scies");
    ensure_directory(&skinny_scies, true)?;
    let ptex_exe = build_context.obtain_ptex(&skinny_scies)?;
    let scie_jump_exe = build_context.obtain_scie_jump(&skinny_scies)?;
    Ok(SkinnyScieTools {
        ptex: ptex_exe,
        scie_jump: scie_jump_exe,
    })
}

fn build_tools_pex(
    build_context: &BuildContext,
    skinny_scie_tools: &SkinnyScieTools,
    update_lock: bool,
) -> Result<PathBuf, Exit> {
    build_step!("Executing scie-jump boot-pack of the `pbt` helper binary");
    let pbt_package_dir = build_context.cargo_output_root.join("pbt");
    ensure_directory(&pbt_package_dir, true)?;

    let pbt_exe = pbt_package_dir
        .join("pbt")
        .with_extension(env::consts::EXE_EXTENSION);
    let scie_jump_dst = pbt_package_dir.join(skinny_scie_tools.scie_jump.file_name().unwrap());
    let ptex_dst = pbt_package_dir.join(skinny_scie_tools.ptex.file_name().unwrap());
    let pbt_manifest = build_context.package_crate_root.join("pbt.lift.json");
    let pbt_manifest_dst = pbt_package_dir.join("lift.json");
    hardlink(&skinny_scie_tools.scie_jump, &scie_jump_dst)?;
    hardlink(&skinny_scie_tools.ptex, &ptex_dst)?;
    hardlink(&pbt_manifest, &pbt_manifest_dst)?;

    execute(Command::new(&skinny_scie_tools.scie_jump).current_dir(&pbt_package_dir))?;

    let tools_path = build_context.workspace_root.join("tools");
    let lock_path = tools_path.join("lock.json");
    let lock = path_as_str(&lock_path)?;
    let requirements_path = tools_path.join("requirements.txt");
    let requirements = path_as_str(&requirements_path)?;
    let test_requirements_path = tools_path.join("test-requirements.txt");
    let test_requirements = path_as_str(&test_requirements_path)?;
    let interpreter_constraints = ["--interpreter-constraint", "CPython>=3.8,<3.10"];

    if update_lock {
        build_step!("Updating the scie_jump tools lock file");
        execute(
            Command::new(&pbt_exe).args(
                [
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
                    "-o",
                    lock,
                    "--indent",
                    "2",
                    "-r",
                    requirements,
                    "-r",
                    test_requirements,
                ]
                .iter()
                .chain(interpreter_constraints.iter()),
            ),
        )?;
    }

    build_step!("Building the scie_pants `tools.pex`");
    let tools_src_path = tools_path.join("src");
    let tools_src = path_as_str(&tools_src_path)?;
    let tools_pex_path = build_context.cargo_output_root.join("tools.pex");
    let tools_pex = path_as_str(&tools_pex_path)?;
    execute(
        Command::new(&pbt_exe).args(
            [
                "pex",
                "--disable-cache",
                "--no-emit-warnings",
                "--lock",
                lock,
                "-r",
                requirements,
                "-c",
                "conscript",
                "-o",
                tools_pex,
                "--venv",
                "prepend",
                "-D",
                tools_src,
            ]
            .iter()
            .chain(interpreter_constraints.iter()),
        ),
    )?;

    Ok(tools_pex_path)
}

fn build_scie_pants_scie(
    build_context: &BuildContext,
    skinny_scie_tools: &SkinnyScieTools,
    tools_pex_file: &Path,
) -> Result<PathBuf, Exit> {
    build_step!("Building the scie-pants Rust binary.");
    execute(
        Command::new(CARGO)
            .args([
                "install",
                "--path",
                path_as_str(&build_context.workspace_root)?,
                "--target",
                &build_context.target,
                "--root",
                path_as_str(&build_context.cargo_output_root)?,
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
    let scie_pants_exe = build_context
        .cargo_output_bin_dir
        .join(BINARY)
        .with_extension(env::consts::EXE_EXTENSION);

    build_step!("Building the `scie-pants` scie");

    // Setup the scie-pants boot-pack.
    let scie_pants_package_dir = build_context.cargo_output_root.join("scie-pants");
    ensure_directory(&scie_pants_package_dir, true)?;

    let scie_jump_dst = scie_pants_package_dir.join("scie-jump");
    let ptex_dst = scie_pants_package_dir.join("ptex");
    // N.B.: We name the scie-pants binary scie-pants.bin since the scie itself is named scie-pants
    // which would conflict when packaging.
    let scie_pants_dst = scie_pants_package_dir.join("scie-pants.bin");
    let tools_pex_dst = scie_pants_package_dir.join("tools.pex");
    let scie_pants_manifest = build_context
        .package_crate_root
        .join("scie-pants.lift.json");
    let scie_pants_manifest_dst = scie_pants_package_dir.join("lift.json");
    hardlink(&skinny_scie_tools.scie_jump, &scie_jump_dst)?;
    hardlink(&skinny_scie_tools.ptex, &ptex_dst)?;
    hardlink(&scie_pants_exe, &scie_pants_dst)?;
    hardlink(tools_pex_file, &tools_pex_dst)?;
    hardlink(&scie_pants_manifest, &scie_pants_manifest_dst)?;

    // Run the boot-pack.
    execute(Command::new(&scie_jump_dst).current_dir(&scie_pants_package_dir))?;
    let scie_pants_scie = scie_pants_package_dir
        .join(BINARY)
        .with_extension(env::consts::EXE_EXTENSION);
    let scie_pants_scie_with_platform = scie_pants_package_dir.join(binary_full_name(BINARY));
    rename(&scie_pants_scie, &scie_pants_scie_with_platform)?;
    Ok(scie_pants_scie_with_platform)
}

fn test(
    workspace_root: &Path,
    tools_pex_path: &Path,
    scie_pants_scie: &Path,
    tools_pex_mismatch_warn: bool,
) -> ExitResult {
    build_step!("Running smoke tests");
    log!(
        Color::Yellow,
        "Disabling pants rc files for the smoke tests."
    );
    env::set_var("PANTS_PANTSRC", "False");

    // Max Python supported is 3.9 and only Linux x86_64 and macOS aarch64 and x86_64 wheels were
    // released.
    if matches!(
        *CURRENT_PLATFORM,
        Platform::LinuxX86_64 | Platform::MacOSAarch64 | Platform::MacOSX86_64
    ) {
        integration_test!("Linting, testing and packaging the tools codebase");
        execute(
            Command::new(scie_pants_scie)
                .args(["fmt", "lint", "check", "test", "package", "::"])
                .env("PEX_SCRIPT", "Does not exist!"),
        )?;

        integration_test!("Checking .pants.bootstrap handling ignores bash functions");
        // N.B.: We run this test after 1st having run the test above to ensure pants is already
        // bootstrapped so that we don't get stderr output from that process. We also use
        // `--no-pantsd` to avoid spurious pantsd startup stderr log lines just in case pantsd found
        // a need to restart.
        let output = execute(
            Command::new(scie_pants_scie)
                .args(["--no-pantsd", "-V"])
                .stderr(Stdio::piped()),
        )?;
        assert!(
            output.stderr.is_empty(),
            "Expected no warnings to be printed when handling .pants.bootstrap, found:\n{warnings}",
            warnings = String::from_utf8_lossy(&output.stderr)
        );

        integration_test!(
            "Verifying the tools.pex built by the package crate matches the tools.pex built by \
            Pants"
        );
        let pants_tools_pex_path = workspace_root.join("dist").join("tools").join("tools.pex");
        let pants_tools_pex_fingerprint = fingerprint(&pants_tools_pex_path)?;
        let our_tools_pex_fingerprint = fingerprint(tools_pex_path)?;
        if !tools_pex_mismatch_warn {
            assert_eq!(our_tools_pex_fingerprint, pants_tools_pex_fingerprint);
        } else if our_tools_pex_fingerprint != pants_tools_pex_fingerprint {
            log!(
                Color::Yellow,
                "The tools.pex generated by Pants does not match ours:{eol}\
                Ours:  {our_tools_path}{eol}\
                ->     {ours}{eol}\
                Pants: {pants_tools_path}{eol}\
                ->     {pants}{eol}",
                our_tools_path = tools_pex_path.display(),
                ours = our_tools_pex_fingerprint,
                pants_tools_path = pants_tools_pex_path.display(),
                pants = pants_tools_pex_fingerprint,
                eol = EOL,
            );
        }

        integration_test!("Verifying PANTS_BOOTSTRAP_TOOLS works");
        execute(
            Command::new(scie_pants_scie)
                .env("PANTS_BOOTSTRAP_TOOLS", "1")
                .args(["bootstrap-cache-key"]),
        )?;

        // TODO(John Sirois): The --no-pantsd here works around a fairly prevalent Pants crash on
        // Linux x86_64 along the lines of the following, but sometimes varying:
        // >> Verifying PANTS_SHA is respected
        // Bootstrapping Pants 2.14.0a0+git8e381dbf using cpython 3.9.15
        // Installing pantsbuild.pants==2.14.0a0+git8e381dbf into a virtual environment at /home/runner/.cache/nce/67f27582b3729c677922eb30c5c6e210aa54badc854450e735ef41cf25ac747f/bindings/venvs/2.14.0a0+git8e381dbf
        // New virtual environment successfully created at /home/runner/.cache/nce/67f27582b3729c677922eb30c5c6e210aa54badc854450e735ef41cf25ac747f/bindings/venvs/2.14.0a0+git8e381dbf.
        // 18:11:53.75 [INFO] Initializing scheduler...
        // 18:11:53.97 [INFO] Scheduler initialized.
        // 2.14.0a0+git8e381dbf
        // Fatal Python error: PyGILState_Release: thread state 0x7efe18001140 must be current when releasing
        // Python runtime state: finalizing (tstate=0x1f4b810)
        //
        // Thread 0x00007efe30b75540 (most recent call first):
        // <no Python frame>
        // Error: Command "/home/runner/work/scie-pants/scie-pants/dist/scie-pants-linux-x86_64" "--no-verify-config" "-V" failed with exit code: None
        if matches!(*CURRENT_PLATFORM, Platform::LinuxX86_64) {
            log!(Color::Yellow, "Turning off pantsd for remaining tests.");
            env::set_var("PANTS_PANTSD", "False");
        }

        integration_test!("Verifying PANTS_SHA is respected");
        execute(
            Command::new(scie_pants_scie)
                .env("PANTS_SHA", "8e381dbf90cae57c5da2b223c577b36ca86cace9")
                .args(["--no-verify-config", "-V"]),
        )?;

        integration_test!(
            "Verifying --python-repos-repos is used prior to Pants 2.13 (no warnings should be \
            issued by Pants)"
        );
        execute(
            Command::new(scie_pants_scie)
                .env("PANTS_VERSION", "2.12.1")
                .args(["--no-verify-config", "-V"]),
        )?;

        integration_test!("Verifying initializing a new Pants project works");
        let new_project_dir = create_tempdir()?;
        execute(Command::new("git").arg("init").arg(new_project_dir.path()))?;
        let project_subdir = new_project_dir.path().join("subdir").join("sub-subdir");
        ensure_directory(&project_subdir, false)?;
        execute_with_input(
            Command::new(scie_pants_scie)
                .arg("-V")
                .current_dir(project_subdir),
            "yes".as_bytes(),
        )?;
        assert!(new_project_dir.path().join("pants.toml").is_file());

        integration_test!("Verifying setting the Pants version on an existing Pants project works");
        let existing_project_dir = create_tempdir()?;
        touch(&existing_project_dir.path().join("pants.toml"))?;
        execute_with_input(
            Command::new(scie_pants_scie)
                .arg("-V")
                .current_dir(existing_project_dir.path()),
            "Y".as_bytes(),
        )?;

        integration_test!(
            "Verify scie-pants can be used as `pants` in a repo with the `pants` script"
        );
        // This verifies a fix for https://github.com/pantsbuild/scie-pants/issues/28.
        let clone_root = create_tempdir()?;
        execute(
            Command::new("git")
                .args(["clone", "https://github.com/pantsbuild/example-django"])
                .current_dir(clone_root.path()),
        )?;
        let bin_dir = clone_root.path().join("bin");
        ensure_directory(&bin_dir, false)?;
        copy(scie_pants_scie, bin_dir.join("pants").as_path())?;
        let new_path = if let Ok(existing_path) = env::var("PATH") {
            format!(
                "{bin_dir}{path_sep}{existing_path}",
                bin_dir = bin_dir.display(),
                path_sep = PATHSEP
            )
        } else {
            format!("{bin_dir}", bin_dir = bin_dir.display())
        };
        execute(
            Command::new("pants")
                .arg("-V")
                .env("PATH", new_path)
                .current_dir(clone_root.path().join("example-django")),
        )?;

        integration_test!(
            "Verify `.env` loading works (example-django should down grade to Pants 2.12.1)"
        );
        write_file(
            &clone_root.path().join(".env"),
            false,
            "PANTS_VERSION=2.12.1",
        )?;
        execute(
            Command::new(scie_pants_scie)
                .arg("-V")
                .current_dir(clone_root.path().join("example-django")),
        )?;

        integration_test!("Verify PANTS_SOURCE mode.");
        let dev_cache_dir = dev_cache_dir()?;
        let clone_dir = dev_cache_dir.join("clones");
        let pants_2_14_1_clone_dir = clone_dir.join("pants-2.14.1");
        let venv_dir = dev_cache_dir.join("venvs");
        let pants_2_14_1_venv_dir = venv_dir.join("pants-2.14.1");
        if !pants_2_14_1_clone_dir.exists() || !pants_2_14_1_venv_dir.exists() {
            let clone_root_tmp = create_tempdir()?;
            let clone_root_path = clone_root_tmp.path().to_str().ok_or_else(|| {
                Code::FAILURE.with_message(format!(
                    "Failed to convert clone root path to UTF-8 string: {clone_root:?}"
                ))
            })?;
            execute(Command::new("git").args(["init", clone_root_path]))?;
            // N.B.: The release_2.14.1 tag has sha cfcb23a97434405a22537e584a0f4f26b4f2993b and we
            // must pass a full sha to use the shallow fetch trick.
            const PANTS_2_14_1_SHA: &str = "cfcb23a97434405a22537e584a0f4f26b4f2993b";
            execute(
                Command::new("git")
                    .args([
                        "fetch",
                        "--depth",
                        "1",
                        "https://github.com/pantsbuild/pants",
                        PANTS_2_14_1_SHA,
                    ])
                    .current_dir(clone_root_tmp.path()),
            )?;
            execute(
                Command::new("git")
                    .args(["reset", "--hard", PANTS_2_14_1_SHA])
                    .current_dir(clone_root_tmp.path()),
            )?;
            write_file(
                clone_root_tmp.path().join("patch").as_path(),
                false,
                r#"
diff --git a/build-support/pants_venv b/build-support/pants_venv
index 81e3bd7..4236f4b 100755
--- a/build-support/pants_venv
+++ b/build-support/pants_venv
@@ -14,11 +14,13 @@ REQUIREMENTS=(
 # NB: We house these outside the working copy to avoid needing to gitignore them, but also to
 # dodge https://github.com/hashicorp/vagrant/issues/12057.
 platform=$(uname -mps | sed 's/ /./g')
-venv_dir_prefix="${HOME}/.cache/pants/pants_dev_deps/${platform}"
+venv_dir_prefix="${PANTS_VENV_DIR_PREFIX:-${HOME}/.cache/pants/pants_dev_deps/${platform}}"
+
+echo >&2 "The ${SCIE_PANTS_TEST_MODE:-Pants 2.14.1 clone} is working."

 function venv_dir() {
   py_venv_version=$(${PY} -c 'import sys; print("".join(map(str, sys.version_info[0:2])))')
-  echo "${venv_dir_prefix}.py${py_venv_version}.venv"
+  echo "${venv_dir_prefix}/py${py_venv_version}.venv"
 }

 function activate_venv() {
diff --git a/pants b/pants
index b422eff..16f0cf5 100755
--- a/pants
+++ b/pants
@@ -70,4 +70,5 @@ function exec_pants_bare() {
     exec ${PANTS_PREPEND_ARGS:-} "$(venv_dir)/bin/python" ${DEBUG_ARGS} "${PANTS_PY_EXE}" "$@"
 }

+echo >&2 "Pants from sources argv: $@."
 exec_pants_bare "$@"
diff --git a/pants.toml b/pants.toml
index ab5cba1..8432bb2 100644
--- a/pants.toml
+++ b/pants.toml
@@ -1,3 +1,6 @@
+[DEFAULT]
+delegate_bootstrap = true
+
 [GLOBAL]
 print_stacktrace = true

diff --git a/src/python/pants/VERSION b/src/python/pants/VERSION
index b70ae75..271706a 100644
--- a/src/python/pants/VERSION
+++ b/src/python/pants/VERSION
@@ -1 +1 @@
-2.14.1
+2.14.1+Custom-Local
\ No newline at end of file
"#,
            )?;
            execute(
                Command::new("git")
                    .args(["apply", "patch"])
                    .current_dir(clone_root_tmp.path()),
            )?;
            let venv_root_tmp = create_tempdir()?;
            execute(
                Command::new("./pants")
                    .arg("-V")
                    .env("PANTS_VENV_DIR_PREFIX", venv_root_tmp.path())
                    .current_dir(clone_root_tmp.path()),
            )?;

            remove_dir(
                clone_root_tmp
                    .path()
                    .join("src")
                    .join("rust")
                    .join("engine")
                    .join("target")
                    .as_path(),
            )?;
            ensure_directory(&clone_dir, true)?;
            rename(&clone_root_tmp.into_path(), &pants_2_14_1_clone_dir)?;
            ensure_directory(&venv_dir, true)?;
            rename(&venv_root_tmp.into_path(), &pants_2_14_1_venv_dir)?;
        }

        let test_pants_from_sources = |command: &mut Command, expected_message: &str| {
            let result = execute(
                command
                    .arg("-V")
                    .env("PANTS_VENV_DIR_PREFIX", &pants_2_14_1_venv_dir)
                    .stderr(Stdio::piped()),
            )?;
            let stderr = String::from_utf8(result.stderr).map_err(|e| {
                Code::FAILURE.with_message(format!("Failed to decode Pants stderr: {e}"))
            })?;
            assert!(
                stderr.contains(expected_message),
                "STDERR did not contain '{expected_message}':\n{stderr}"
            );
            let expected_argv_message = "Pants from sources argv: --no-verify-config -V.";
            assert!(
                stderr.contains(expected_argv_message),
                "STDERR did not contain '{expected_argv_message}':\n{stderr}"
            );
            Ok(())
        };

        test_pants_from_sources(
            Command::new(scie_pants_scie)
                .env("PANTS_SOURCE", &pants_2_14_1_clone_dir)
                .env("SCIE_PANTS_TEST_MODE", "PANTS_SOURCE mode"),
            "The PANTS_SOURCE mode is working.",
        )?;

        integration_test!("Verify pants_from_sources mode.");
        let side_by_side_root = create_tempdir()?;
        let pants_dir = side_by_side_root.path().join("pants");
        softlink(&pants_2_14_1_clone_dir, &pants_dir)?;
        let user_repo_dir = side_by_side_root.path().join("user-repo");
        ensure_directory(&user_repo_dir, true)?;
        touch(user_repo_dir.join("pants.toml").as_path())?;
        touch(user_repo_dir.join("BUILD_ROOT").as_path())?;

        let pants_from_sources = side_by_side_root.path().join("pants_from_sources");
        softlink(scie_pants_scie, &pants_from_sources)?;

        test_pants_from_sources(
            Command::new(pants_from_sources)
                .env("SCIE_PANTS_TEST_MODE", "pants_from_sources mode")
                .current_dir(user_repo_dir),
            "The pants_from_sources mode is working.",
        )?;

        integration_test!("Verify delegating to `./pants`.");
        let result = execute(
            Command::new(scie_pants_scie)
                .arg("-V")
                .env("SCIE_PANTS_TEST_MODE", "delegate_bootstrap mode")
                .current_dir(pants_2_14_1_clone_dir)
                .stderr(Stdio::piped()),
        )?;
        let stderr = String::from_utf8(result.stderr).map_err(|e| {
            Code::FAILURE.with_message(format!("Failed to decode Pants stderr: {e}"))
        })?;
        let expected_message = "The delegate_bootstrap mode is working.";
        assert!(
            stderr.contains(expected_message),
            "STDERR did not contain '{expected_message}':\n{stderr}"
        );
        let expected_argv_message = "Pants from sources argv: -V.";
        assert!(
            stderr.contains(expected_argv_message),
            "STDERR did not contain '{expected_argv_message}':\n{stderr}"
        );
    }

    // Max Python supported is 3.8 and only Linux and macOS x86_64 wheels were released.
    if matches!(
        *CURRENT_PLATFORM,
        Platform::LinuxX86_64 | Platform::MacOSX86_64
    ) {
        integration_test!("Verifying Python 3.8 is selected for Pants older than 2.5.0");
        let mut command = Command::new(scie_pants_scie);
        command
            .env("PANTS_VERSION", "1.30.5rc1")
            .env(
                "PANTS_BACKEND_PACKAGES",
                "-[\
                'pants.backend.python.typecheck.mypy',\
                'pants.backend.shell',\
                'pants.backend.shell.lint.shellcheck',\
                'pants.backend.shell.lint.shfmt',\
                ]",
            )
            .args(["--no-verify-config", "--version"]);
        if Platform::MacOSX86_64 == *CURRENT_PLATFORM {
            // For unknown reasons, macOS x86_64 hangs in CI if this last test, like all prior tests
            // nonetheless!, is run with pantsd enabled mode.
            command.arg("--no-pantsd");
        }
        execute(&mut command)?;
    }

    integration_test!("Verifying self update works");
    // N.B.: There should never be a newer release in CI; so this should always gracefully noop
    // noting no newer release was available.
    execute(Command::new(scie_pants_scie).env("SCIE_BOOT", "update"))?;

    integration_test!("Verifying downgrade works");
    // Additionally, we exercise using a relative path to the scie-jump binary which triggered
    // https://github.com/pantsbuild/scie-pants/issues/38 in the past.
    execute(
        Command::new(PathBuf::from(".").join(scie_pants_scie.file_name().unwrap()))
            .env("SCIE_BOOT", "update")
            .arg("0.1.8")
            .current_dir(scie_pants_scie.parent().unwrap()),
    )?;

    Ok(())
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

#[derive(Subcommand)]
enum Commands {
    /// Builds the `tools.pex` used by the scie-pants scie to perform Pants installs.
    Tools,
    /// Builds the `scie-pants` scie.
    Scie {
        #[arg(
            long,
            help = "The location of the pre-built tools.pex to use. By default, the tools.pex is \
            built fresh."
        )]
        tools_pex: Option<PathBuf>,
    },
    /// Builds the `scie-pants` scie and runs it through a series of integration tests.
    Test {
        #[arg(
            long,
            help = "The location of the pre-built tools.pex to use. By default, the tools.pex is \
            built fresh."
        )]
        tools_pex: Option<PathBuf>,
        #[arg(
            long,
            help = "The location of the pre-built scie-pants scie to use. By default, the \
            scie-pants scie is built fresh."
        )]
        scie_pants: Option<PathBuf>,
        #[arg(
            long,
            help = "Only warn if the Pants built tools.pex doesn't match ours instead of failing \
            the tests.",
            default_value_t = false
        )]
        tools_pex_mismatch_warn: bool,
    },
}

#[derive(Parser)]
#[command(about, version)]
struct Args {
    #[arg(long, help = "Override the default --target for this platform.")]
    target: Option<String>,
    #[arg(
        long,
        help = format!(
            "Instead of using the released {PTEX_TAG} ptex, package ptex from the ptex project \
            repo at this directory."
        )
    )]
    ptex: Option<PathBuf>,
    #[arg(
        long,
        help = format!(
            "Instead of using the released {SCIE_JUMP_TAG} scie-jump, package the scie-jump from \
            the scie-jump project repo at this directory."
        )
    )]
    scie_jump: Option<PathBuf>,
    #[arg(
        long,
        help = "Refresh the tools lock before building the tools.pex",
        default_value_t = false
    )]
    update_lock: bool,
    #[arg(
        long,
        help = "The destination directory for the chosen binary and its checksum file.",
        default_value_t = SpecifiedPath::new("dist")
    )]
    dest_dir: SpecifiedPath,
    #[command(subcommand)]
    command: Commands,
}

fn maybe_build(args: &Args, build_context: &BuildContext) -> Result<Option<PathBuf>, Exit> {
    match &args.command {
        Commands::Test {
            tools_pex: Some(tools_pex),
            scie_pants: Some(scie_pants),
            tools_pex_mismatch_warn,
        } => {
            test(
                &build_context.workspace_root,
                &canonicalize(tools_pex)?,
                &canonicalize(scie_pants)?,
                *tools_pex_mismatch_warn,
            )?;
            Ok(None)
        }
        Commands::Test {
            tools_pex: None,
            scie_pants: Some(scie_pants),
            tools_pex_mismatch_warn,
        } => {
            let skinny_scie_tools = fetch_skinny_scie_tools(build_context)?;
            let tools_pex = build_tools_pex(build_context, &skinny_scie_tools, args.update_lock)?;
            test(
                &build_context.workspace_root,
                &tools_pex,
                &canonicalize(scie_pants)?,
                *tools_pex_mismatch_warn,
            )?;
            Ok(None)
        }
        Commands::Test {
            tools_pex: Some(tools_pex),
            scie_pants: None,
            tools_pex_mismatch_warn,
        } => {
            let skinny_scie_tools = fetch_skinny_scie_tools(build_context)?;
            let scie_pants = build_scie_pants_scie(build_context, &skinny_scie_tools, tools_pex)?;
            test(
                &build_context.workspace_root,
                &canonicalize(tools_pex)?,
                &scie_pants,
                *tools_pex_mismatch_warn,
            )?;
            Ok(Some(scie_pants))
        }
        Commands::Test {
            tools_pex: None,
            scie_pants: None,
            tools_pex_mismatch_warn,
        } => {
            let skinny_scie_tools = fetch_skinny_scie_tools(build_context)?;
            let tools_pex = build_tools_pex(build_context, &skinny_scie_tools, args.update_lock)?;
            let scie_pants = build_scie_pants_scie(build_context, &skinny_scie_tools, &tools_pex)?;
            test(
                &build_context.workspace_root,
                &tools_pex,
                &scie_pants,
                *tools_pex_mismatch_warn,
            )?;
            Ok(Some(scie_pants))
        }
        Commands::Scie { tools_pex: None } => {
            let skinny_scie_tools = fetch_skinny_scie_tools(build_context)?;
            let tools_pex = build_tools_pex(build_context, &skinny_scie_tools, args.update_lock)?;
            Ok(Some(build_scie_pants_scie(
                build_context,
                &skinny_scie_tools,
                &tools_pex,
            )?))
        }
        Commands::Scie {
            tools_pex: Some(tools_pex),
        } => {
            let skinny_scie_tools = fetch_skinny_scie_tools(build_context)?;
            Ok(Some(build_scie_pants_scie(
                build_context,
                &skinny_scie_tools,
                tools_pex,
            )?))
        }
        Commands::Tools => {
            let skinny_scie_tools = fetch_skinny_scie_tools(build_context)?;
            Ok(Some(build_tools_pex(
                build_context,
                &skinny_scie_tools,
                args.update_lock,
            )?))
        }
    }
}

fn main() -> ExitResult {
    pretty_env_logger::init();

    let args = Args::parse();

    let dest_dir = &args.dest_dir;
    if dest_dir.is_file() {
        return Err(Code::FAILURE.with_message(format!(
            "The specified dest_dir of {} is a file. Not overwriting",
            dest_dir.display()
        )));
    }

    let build_context = BuildContext::new(
        args.target.as_deref(),
        args.ptex.as_deref(),
        args.scie_jump.as_deref(),
    )?;
    if let Some(output_file) = maybe_build(&args, &build_context)? {
        let dest_file_name = output_file
            .file_name()
            .ok_or_else(|| {
                Code::FAILURE.with_message(format!(
                    "Failed to determine the basename of {output_file:?}"
                ))
            })?
            .to_str()
            .ok_or_else(|| {
                Code::FAILURE.with_message(format!(
                    "Failed to interpret the basename of {output_file:?} as a UTF-8 string"
                ))
            })?;
        let dest_file = dest_dir.join(dest_file_name);
        ensure_directory(dest_dir, false)?;
        copy(&output_file, &dest_file)?;

        let fingerprint_file = dest_file.with_file_name(format!("{dest_file_name}.sha256"));
        let dest_file_digest = fingerprint(&dest_file)?;
        std::fs::write(
            &fingerprint_file,
            format!("{dest_file_digest} *{dest_file_name}\n"),
        )
        .map_err(|e| {
            Code::FAILURE.with_message(format!(
                "Failed to write fingerprint file {fingerprint_file}: {e}",
                fingerprint_file = fingerprint_file.display()
            ))
        })?;
        check_sha256(&dest_file)?;

        log!(
            Color::Yellow,
            "Wrote {dest_file_name} to {dest_file}",
            dest_file = dest_file.display()
        );
    }

    Ok(())
}
