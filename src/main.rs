// Copyright 2022 Pants project contributors.
// Licensed under the Apache License, Version 2.0 (see LICENSE).

use std::env;
use std::ffi::OsString;

use anyhow::{anyhow, Context, Result};
use build_root::BuildRoot;
use log::{info, trace};
use logging_timer::{time, timer, Level};
use pyver::PackageVersion;

use crate::config::PantsConfig;
use crate::pants_bootstrap::PantsBootstrap;

mod build_root;
mod config;
mod pants_bootstrap;

#[derive(Debug, Default)]
struct Process {
    exe: OsString,
    args: Vec<OsString>,
    env: Vec<(OsString, OsString)>,
}

impl Process {
    #[cfg(not(target_family = "unix"))]
    fn exec(self) -> Result<i32> {
        use std::process::Command;

        let exit_status = Command::new(&self.exe)
            .args(&self.args)
            .args(std::env::args().skip(1))
            .envs(self.env.clone())
            .spawn()?
            .wait()
            .with_context(|| format!("Failed to execute process: {self:#?}"))?;
        Ok(exit_status
            .code()
            .unwrap_or_else(|| if exit_status.success() { 0 } else { 1 }))
    }

    #[cfg(target_family = "unix")]
    fn exec(self) -> Result<i32> {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStringExt;

        use nix::unistd::execv;

        let c_exe = CString::new(self.exe.into_vec())
            .context("Failed to convert executable to a C string.")?;

        let mut c_args = vec![c_exe.clone()];
        c_args.extend(
            self.args
                .into_iter()
                .chain(std::env::args().skip(1).map(OsString::from))
                .map(|arg| {
                    CString::new(arg.into_vec())
                        .context("Failed to convert argument to a C string.")
                })
                .collect::<Result<Vec<_>, _>>()?,
        );

        for (name, value) in self.env {
            std::env::set_var(name, value);
        }

        execv(&c_exe, &c_args)
            .map(|_| 0)
            .context("Failed to exec process.")
    }
}

#[time("debug", "ptex::{}")]
fn get_pants_process() -> Result<Process> {
    let build_root = BuildRoot::find(None)?;
    if let Some(pants_bootstrap) = PantsBootstrap::load(&build_root)? {
        pants_bootstrap.export_env();
    }
    let pants_config = PantsConfig::parse(build_root)?;
    let build_root = pants_config.build_root().to_path_buf();

    let env_version = if let Some(raw_version) = env::var_os("PANTS_VERSION") {
        Some(PackageVersion::new(
            raw_version
                .into_string()
                .map_err(|raw| {
                    anyhow!("Failed to interpret PANTS_VERSION {raw:?} as UTF-8 string.")
                })?
                .as_str(),
        )?)
    } else {
        None
    };
    let pants_version = if let Some(ref env_version) = env_version {
        Some(env_version)
    } else {
        pants_config.package_version()
    };

    let debugpy_version = pants_config.debugpy_version().unwrap_or_default();

    let python = if let Some(version) = pants_version {
        match (version.release.major, version.release.minor) {
            (1, _) => "python3.8",
            (2, minor) if minor < 5 => "python3.8",
            _ => "python3.9",
        }
    } else {
        "python3.9"
    };

    info!(
        "Found Pants build root at {build_root}",
        build_root = build_root.display()
    );
    info!("The required Pants version is {pants_version:?}");

    let scie =
        env::var_os("SCIE").context("Failed to retrieve SCIE location from the environment.")?;
    let scie_argv0 = env::var_os("SCIE_ARGV0")
        .context("Failed to retrieve SCIE_ARGV0 location from the environment.")?;

    let (scie_boot, pants_debug) = match env::var_os("PANTS_DEBUG") {
        Some(value) if !value.is_empty() => ("pants_debug", "1"),
        _ => ("pants", ""),
    };

    Ok(Process {
        exe: scie,
        env: vec![
            ("SCIE_BOOT".into(), scie_boot.into()),
            ("PANTS_BIN_NAME".into(), scie_argv0),
            (
                "PANTS_BUILDROOT_OVERRIDE".into(),
                build_root.into_os_string(),
            ),
            ("PANTS_DEBUG".into(), pants_debug.into()),
            ("PANTS_DEBUGPY_VERSION".into(), debugpy_version.into()),
            (
                "PANTS_VERSION".into(),
                pants_version
                    .map(|pv| pv.original.clone())
                    .unwrap_or_default()
                    .into(),
            ),
            ("PYTHON".into(), python.into()),
        ],
        ..Default::default()
    })
}

trait OrExit<T> {
    fn or_exit(self) -> T;
}

impl<T> OrExit<T> for Result<T> {
    fn or_exit(self) -> T {
        match self {
            Ok(item) => item,
            Err(err) => {
                eprintln!("{:#}", err);
                std::process::exit(1)
            }
        }
    }
}

fn main() {
    env_logger::init();
    let _timer = timer!(Level::Debug; "MAIN");
    let pants_process = get_pants_process().or_exit();
    trace!("Launching: {pants_process:#?}");
    let exit_code = pants_process.exec().or_exit();
    std::process::exit(exit_code)
}
