// Copyright 2022 Pants project contributors.
// Licensed under the Apache License, Version 2.0 (see LICENSE).

use std::ffi::OsString;

use anyhow::Result;
use build_root::BuildRoot;
use log::{info, warn};
use logging_timer::{time, timer, Level};

use crate::config::PantsConfig;
use crate::pants_bootstrap::PantsBootstrap;

mod build_root;
mod config;
mod pants_bootstrap;

#[derive(Debug, Default)]
struct Process {
    _exe: OsString,
    _args: Vec<OsString>,
    _env: Vec<(OsString, OsString)>,
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
    Ok(Process {
        _exe: "./pants".into(),
        ..Default::default()
    })
}

fn main() {
    env_logger::init();
    let _timer = timer!(Level::Debug; "MAIN");
    let pants_process = match get_pants_process() {
        Ok(process) => process,
        Err(err) => {
            eprintln!("{:#}", err);
            std::process::exit(1);
        }
    };
    warn!("Should execute {pants_process:#?}");
}
