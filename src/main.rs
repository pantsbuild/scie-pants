// Copyright 2022 Pants project contributors.
// Licensed under the Apache License, Version 2.0 (see LICENSE).

use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt::Debug;
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use build_root::BuildRoot;
use log::{info, trace};
use logging_timer::{time, timer, Level};
use uuid::Uuid;

use crate::config::PantsConfig;

mod build_root;
mod config;

const SCIE_PANTS_VERSION: &str = env!("CARGO_PKG_VERSION");

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

fn env_version(env_var_name: &str) -> Result<Option<String>> {
    let raw_version = env::var_os(env_var_name).unwrap_or(OsString::new());
    if raw_version.len() == 0 {
        // setting PANTS_VERSION= behaves the same as not setting it
        Ok(None)
    } else {
        Ok(Some(raw_version.into_string().map_err(|raw| {
            anyhow!("Failed to interpret {env_var_name} {raw:?} as UTF-8 string.")
        })?))
    }
}

fn find_pants_installation() -> Result<Option<PantsConfig>> {
    if let Ok(build_root) = BuildRoot::find(None) {
        let pants_config = PantsConfig::parse(build_root)?;
        return Ok(Some(pants_config));
    }
    Ok(None)
}

#[derive(Eq, PartialEq)]
enum ScieBoot {
    BootstrapTools,
    Pants,
    PantsDebug,
}

impl ScieBoot {
    fn env_value(&self) -> OsString {
        match self {
            ScieBoot::BootstrapTools => "bootstrap-tools",
            ScieBoot::Pants => "pants",
            ScieBoot::PantsDebug => "pants-debug",
        }
        .into()
    }

    #[cfg(unix)]
    fn quote<T: Into<OsString> + Debug>(value: T) -> Result<String> {
        String::from_utf8(shell_quote::bash::escape(value))
            .context("Shell-quoted value could not be interpreted as UTF-8.")
    }

    #[cfg(windows)]
    fn quote<T: Into<OsString> + Debug>(_value: T) -> Result<String> {
        // The shell_quote crate assumes unix and fails to compile on Windows.
        todo!("TODO(John Sirois): Figure out Git bash? shell quoting for Windows WTF-16 strings.")
    }

    fn into_process(
        self,
        scie: String,
        build_root: Option<PathBuf>,
        env: Vec<(OsString, OsString)>,
    ) -> Result<Process> {
        Ok(match build_root.map(|br| br.join(".pants.bootstrap")) {
            Some(pants_bootstrap) if self != Self::BootstrapTools && pants_bootstrap.is_file() => {
                Process {
                    exe: "/usr/bin/env".into(),
                    args: vec![
                        "bash".into(),
                        "-c".into(),
                        format!(
                            r#"set -eou pipefail; source {bootstrap}; exec {scie} "$0" "$@""#,
                            bootstrap = Self::quote(pants_bootstrap)?,
                            scie = Self::quote(scie)?
                        )
                        .into(),
                    ],
                    env,
                }
            }
            _ => Process {
                exe: scie.into(),
                env,
                ..Default::default()
            },
        })
    }
}

#[time("debug", "scie-pants::{}")]
fn get_pants_process() -> Result<Process> {
    let pants_installation = find_pants_installation()?;
    let (build_root, configured_pants_version, debugpy_version, delegate_bootstrap) =
        if let Some(ref pants_config) = pants_installation {
            (
                Some(pants_config.build_root().to_path_buf()),
                pants_config.package_version(),
                pants_config.debugpy_version(),
                pants_config.delegate_bootstrap(),
            )
        } else {
            (None, None, None, false)
        };

    let env_pants_version = env_version("PANTS_VERSION")?;
    let pants_version = if let Some(env_version) = env_pants_version {
        Some(env_version)
    } else {
        configured_pants_version.clone()
    };

    if delegate_bootstrap && pants_version.is_none() {
        let exe = build_root
            .expect("Failed to locate build root")
            .join("pants")
            .into_os_string();
        return Ok(Process {
            exe,
            ..Default::default()
        });
    }

    info!("Found Pants build root at {build_root:?}");
    info!("The required Pants version is {pants_version:?}");

    let scie =
        env::var("SCIE").context("Failed to retrieve SCIE location from the environment.")?;

    let pants_debug = matches!(env::var_os("PANTS_DEBUG"), Some(value) if !value.is_empty());
    let scie_boot = match env::var_os("PANTS_BOOTSTRAP_TOOLS") {
        Some(_) => ScieBoot::BootstrapTools,
        None if pants_debug => ScieBoot::PantsDebug,
        None => ScieBoot::Pants,
    };

    let pants_bin_name = env::var_os("PANTS_BIN_NAME")
        .or_else(|| env::var_os("SCIE_ARGV0"))
        .unwrap_or_else(|| scie.clone().into());

    let mut env = vec![
        ("SCIE_BOOT".into(), scie_boot.env_value()),
        ("PANTS_BIN_NAME".into(), pants_bin_name),
        (
            "PANTS_DEBUG".into(),
            if pants_debug { "1" } else { "" }.into(),
        ),
        ("SCIE_PANTS_VERSION".into(), SCIE_PANTS_VERSION.into()),
    ];
    if let Some(debugpy_version) = debugpy_version {
        env.push(("PANTS_DEBUGPY_VERSION".into(), debugpy_version.into()));
    }
    if let Some(ref build_root) = build_root {
        env.push((
            "PANTS_BUILDROOT_OVERRIDE".into(),
            build_root.as_os_str().to_os_string(),
        ));
        // This should not be conditional. Ideally we'd always set this env var, which is used
        // by the configure binding, and scie-jump would be smart enough to skip the configure
        // binding when the install binding is a cache hit.
        if configured_pants_version.is_none() {
            env.push((
                "PANTS_TOML".into(),
                build_root.join("pants.toml").into_os_string(),
            ));
        }
    }
    if let Some(version) = pants_version {
        if delegate_bootstrap {
            env.push(("_PANTS_VERSION_OVERRIDE".into(), version.clone().into()));
        }
        env.push(("PANTS_VERSION".into(), version.into()));
    } else {
        // Ensure the install binding always re-runs when no Pants version is found so that the
        // the user can be prompted with configuration options.
        env.push((
            "PANTS_VERSION_PROMPT_SALT".into(),
            Uuid::new_v4().simple().to_string().into(),
        ))
    }

    scie_boot.into_process(scie, build_root, env)
}

fn get_pants_from_sources_process(pants_repo_location: PathBuf) -> Result<Process> {
    let exe = pants_repo_location.join("pants").into_os_string();

    let args = vec!["--no-verify-config".into()];

    let version = std::fs::read_to_string(
        pants_repo_location
            .join("src")
            .join("python")
            .join("pants")
            .join("VERSION"),
    )?;

    // The ENABLE_PANTSD env var is a custom env var defined by the legacy `./pants_from_sources`
    // script. We maintain support here in perpetuity because it's cheap and we don't break folks'
    // workflows.
    let enable_pantsd = env::var_os("ENABLE_PANTSD")
        .or_else(|| env::var_os("PANTS_PANTSD"))
        .unwrap_or_else(|| "false".into());

    let build_root = BuildRoot::find(None)?;
    let env = vec![
        ("PANTS_VERSION".into(), version.trim().into()),
        ("PANTS_PANTSD".into(), enable_pantsd),
        (
            "PANTS_BUILDROOT_OVERRIDE".into(),
            build_root.as_os_str().to_os_string(),
        ),
        ("no_proxy".into(), "*".into()),
        ("SCIE_PANTS_VERSION".into(), SCIE_PANTS_VERSION.into()),
    ];

    Ok(Process { exe, args, env })
}

fn invoked_as_basename() -> Option<String> {
    let scie = env::var("SCIE_ARGV0").ok()?;
    let exe_path = PathBuf::from(scie);

    #[cfg(windows)]
    let basename = exe_path.file_stem().and_then(OsStr::to_str);

    #[cfg(unix)]
    let basename = exe_path.file_name().and_then(OsStr::to_str);

    basename.map(str::to_owned)
}

fn main() -> Result<()> {
    env_logger::init();
    let _timer = timer!(Level::Debug; "MAIN");

    // N.B.: The bogus version of `report` is used to signal scie-pants should report version
    // information for the update tool to use in determining if there are newer versions of
    // scie-pants available.
    if let Ok(value) = env::var("PANTS_BOOTSTRAP_VERSION") {
        if "report" == value.as_str() {
            println!("{}", SCIE_PANTS_VERSION);
            std::process::exit(0);
        }
    }

    let pants_process = if let Ok(value) = env::var("PANTS_SOURCE") {
        get_pants_from_sources_process(PathBuf::from(value))
    } else if let Some("pants_from_sources") = invoked_as_basename().as_deref() {
        get_pants_from_sources_process(PathBuf::from("..").join("pants"))
    } else {
        get_pants_process()
    }?;

    trace!("Launching: {pants_process:#?}");
    let exit_code = pants_process.exec()?;
    std::process::exit(exit_code)
}
