// Copyright 2023 Pants project contributors.
// Licensed under the Apache License, Version 2.0 (see LICENSE).

use std::env;
use std::fmt::{Display, Formatter};
use std::fs::Permissions;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output};

use anyhow::{bail, Context, Result};
use lazy_static::lazy_static;
use log::info;

use super::os::EOL;

#[derive(Eq, PartialEq)]
pub(crate) enum Platform {
    LinuxAarch64,
    LinuxX86_64,
    MacOSAarch64,
    MacOSX86_64,
    WindowsX86_64,
}

impl Platform {
    pub(crate) fn current() -> Result<Self> {
        match (env::consts::OS, env::consts::ARCH) {
            ("linux", "aarch64") => Ok(Self::LinuxAarch64),
            ("linux", "x86_64") => Ok(Self::LinuxX86_64),
            ("macos", "aarch64") => Ok(Self::MacOSAarch64),
            ("macos", "x86_64") => Ok(Self::MacOSX86_64),
            ("windows", "x86_64") => Ok(Self::WindowsX86_64),
            _ => bail!(
                "Unsupported platform: os={os} arch={arch}",
                os = env::consts::OS,
                arch = env::consts::ARCH
            ),
        }
    }

    pub(crate) fn to_str(&self) -> &str {
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
    pub(crate) static ref CURRENT_PLATFORM: Platform = Platform::current().unwrap();
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

pub(crate) fn prepare_exe(path: &Path) -> Result<()> {
    if let Some(permissions) = executable_permissions() {
        std::fs::set_permissions(path, permissions).with_context(|| {
            format!("Failed to mark {path} as executable", path = path.display())
        })?
    }
    Ok(())
}

pub(crate) fn execute_with_input(command: &mut Command, stdin_data: &[u8]) -> Result<Output> {
    _execute_with_input(command, Some(stdin_data))
}

pub(crate) fn execute(command: &mut Command) -> Result<Output> {
    _execute_with_input(command, None)
}

fn _execute_with_input(command: &mut Command, stdin_data: Option<&[u8]>) -> Result<Output> {
    info!("Executing {command:#?}");
    if stdin_data.is_some() {
        command.stdin(std::process::Stdio::piped());
    }
    let mut child = command
        .spawn()
        .with_context(|| format!("Failed to spawn command: {command:?}"))?;
    if let Some(stdin_data) = stdin_data {
        child
            .stdin
            .as_mut()
            .expect("We just set a stdin pipe above")
            .write(stdin_data)
            .with_context(|| format!("Failed to write {stdin_data:?} to sub-process stdin"))?;
    }
    let output = child
        .wait_with_output()
        .with_context(|| format!("Failed to gather exit status of command: {command:?}"))?;
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
        bail!(message_lines.join(EOL));
    }
    Ok(output)
}

pub(crate) fn binary_full_name(name: &str) -> String {
    format!(
        "{name}-{platform}{exe}",
        platform = *CURRENT_PLATFORM,
        exe = env::consts::EXE_SUFFIX
    )
}
