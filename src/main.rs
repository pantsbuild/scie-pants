// Copyright 2022 Pants project contributors.
// Licensed under the Apache License, Version 2.0 (see LICENSE).

use std::env;
use std::ffi::OsString;

use anyhow::{anyhow, bail, Context, Result};
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
    #[cfg(windows)]
    fn exec(self) -> Result<i32> {
        use std::process::Command;

        let exit_status = Command::new(&self.exe)
            .args(&self.args)
            .args(env::args().skip(1))
            .envs(self.env.clone())
            .spawn()?
            .wait()
            .with_context(|| format!("Failed to execute process: {self:#?}"))?;
        Ok(exit_status
            .code()
            .unwrap_or_else(|| if exit_status.success() { 0 } else { 1 }))
    }

    #[cfg(unix)]
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
                .chain(env::args().skip(1).map(OsString::from))
                .map(|arg| {
                    CString::new(arg.into_vec())
                        .context("Failed to convert argument to a C string.")
                })
                .collect::<Result<Vec<_>, _>>()?,
        );

        for (name, value) in self.env {
            env::set_var(name, value);
        }

        execv(&c_exe, &c_args)
            .map(|_| 0)
            .context("Failed to exec process.")
    }
}

fn env_version(env_var_name: &str) -> Result<Option<PackageVersion>> {
    let version = if let Some(raw_version) = env::var_os(env_var_name) {
        Some(PackageVersion::new(
            raw_version
                .into_string()
                .map_err(|raw| {
                    anyhow!("Failed to interpret {env_var_name} {raw:?} as UTF-8 string.")
                })?
                .as_str(),
        )?)
    } else {
        None
    };
    Ok(version)
}

#[time("debug", "ptex::{}")]
fn get_pants_process() -> Result<Process> {
    let build_root = BuildRoot::find(None)?;
    if let Some(pants_bootstrap) = PantsBootstrap::load(&build_root)? {
        pants_bootstrap.export_env();
    }
    let pants_config = PantsConfig::parse(build_root)?;
    let build_root = pants_config.build_root().to_path_buf();

    let env_pants_sha = env_version("PANTS_SHA")?;
    let env_pants_version = env_version("PANTS_VERSION")?;
    if let (Some(pants_sha), Some(pants_version)) = (&env_pants_sha, &env_pants_version) {
        bail!(
            "Both PANTS_SHA={pants_sha} and PANTS_VERSION={pants_version} were set. \
            Please choose one.",
            pants_sha = pants_sha.original,
            pants_version = pants_version.original
        )
    }

    let pants_version = if let Some(ref env_version) = env_pants_version {
        Some(env_version)
    } else if env_pants_sha.is_none() {
        pants_config.package_version()
    } else {
        None
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
    info!("Selected {python}");

    let scie =
        env::var_os("SCIE").context("Failed to retrieve SCIE location from the environment.")?;

    let pants_debug = matches!(env::var_os("PANTS_DEBUG"), Some(value) if !value.is_empty());
    let scie_boot = match env::var_os("PANTS_BOOTSTRAP_TOOLS") {
        Some(_) => "bootstrap-tools",
        None => {
            if pants_debug {
                "pants-debug"
            } else {
                "pants"
            }
        }
    };

    let mut env = vec![
        ("SCIE_BOOT".into(), scie_boot.into()),
        ("PANTS_BIN_NAME".into(), scie.as_os_str().into()),
        (
            "PANTS_BUILDROOT_OVERRIDE".into(),
            build_root.into_os_string(),
        ),
        (
            "PANTS_DEBUG".into(),
            if pants_debug { "1" } else { "" }.into(),
        ),
        ("PANTS_DEBUGPY_VERSION".into(), debugpy_version.into()),
        ("PYTHON".into(), python.into()),
    ];
    if let Some(version) = pants_version {
        env.push(("PANTS_VERSION".into(), version.original.clone().into()));
    }

    Ok(Process {
        exe: scie,
        env,
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

    // N.B.: The bogus version of `report` is used to signal scie-pants should report version
    // information for the update tool to use in determining if there are newer versions of
    // scie-pants available.
    if let Ok(value) = env::var("PANTS_BOOTSTRAP_VERSION") {
        if "report" == value.as_str() {
            println!(env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        }
    }

    let pants_process = get_pants_process().or_exit();
    trace!("Launching: {pants_process:#?}");
    let exit_code = pants_process.exec().or_exit();
    std::process::exit(exit_code)
}
