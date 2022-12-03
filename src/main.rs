// Copyright 2022 Pants project contributors.
// Licensed under the Apache License, Version 2.0 (see LICENSE).

use std::env;
use std::ffi::OsString;

use anyhow::{Context, Result};
use build_root::BuildRoot;
use log::info;
use logging_timer::{time, timer, Level};

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

#[time("debug")]
fn get_pants_process() -> Result<Process> {
    let build_root = BuildRoot::find(None)?;
    if let Some(pants_bootstrap) = PantsBootstrap::load(&build_root)? {
        pants_bootstrap.export_env();
    }
    let mut pants_config = PantsConfig::parse(build_root)?;
    let setup_cache = pants_config.get_setup_cache()?;
    info!(
        "Found Pants build root at {build_root} and setup cache at {setup_cache}",
        build_root = pants_config.build_root().display(),
        setup_cache = setup_cache.display()
    );
    info!(
        "The required Pants version is {pants_version}",
        pants_version = pants_config.config.global.pants_version
    );
    let scie =
        env::var_os("SCIE").context("Failed to retrieve SCIE location from the environment.")?;
    Ok(Process {
        exe: scie,
        env: vec![
            ("SCIE_BOOT".into(), "legacy_pants".into()),
            (
                "PANTS_BUILDROOT_OVERRIDE".into(),
                pants_config.build_root().to_path_buf().into_os_string(),
            ),
            ("PANTS_SETUP_CACHE".into(), setup_cache.into_os_string()),
            (
                "PANTS_VERSION".into(),
                pants_config.config.global.pants_version.into(),
            ),
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
    let exit_code = pants_process.exec().or_exit();
    std::process::exit(exit_code)
}
