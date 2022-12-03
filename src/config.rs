// Copyright 2022 Pants project contributors.
// Licensed under the Apache License, Version 2.0 (see LICENSE).

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use logging_timer::time;
use serde::Deserialize;

use crate::build_root::BuildRoot;

#[derive(Deserialize)]
pub(crate) struct Global {
    pub pants_version: String,
}

#[derive(Default, Deserialize)]
pub(crate) struct Setup {
    cache: Option<PathBuf>,
}

impl Setup {
    #[time("debug")]
    pub(crate) fn cache(&mut self) -> Result<PathBuf> {
        if let Some(setup_cache) = self.cache.as_ref() {
            return Ok(setup_cache.clone());
        }

        let default_cache = if let Some(cache_dir) = dirs::cache_dir() {
            cache_dir.join("pants").join("setup")
        } else if let Some(home_dir) = dirs::home_dir() {
            home_dir.join(".pants-setup")
        } else {
            bail!(
                "Failed to determine a reasonable default cache directory for Pants setup and \
                failed to find a fall back user home directory to establish \
                ~/.pants-setup"
            );
        };
        self.cache = Some(default_cache.clone());
        Ok(default_cache)
    }
}

#[derive(Deserialize)]
pub(crate) struct Config {
    #[serde(rename = "GLOBAL")]
    pub(crate) global: Global,
    #[serde(default)]
    pub(crate) setup: Setup,
}

pub(crate) struct PantsConfig {
    build_root: BuildRoot,
    pub(crate) config: Config,
}

impl PantsConfig {
    pub(crate) fn build_root(&self) -> &Path {
        self.build_root.as_path()
    }

    pub(crate) fn get_setup_cache(&mut self) -> Result<PathBuf> {
        self.config.setup.cache()
    }
}

impl PantsConfig {
    #[time("debug")]
    pub(crate) fn parse(build_root: BuildRoot) -> Result<PantsConfig> {
        let pants_config = build_root.join("pants.toml");
        let contents = std::fs::read(&pants_config).with_context(|| {
            format!(
                "Failed to read Pants config from {path}",
                path = pants_config.display()
            )
        })?;
        let config: Config = toml::from_slice(contents.as_slice()).with_context(|| {
            format!(
                "Failed to parse Pants config from {path}",
                path = pants_config.display()
            )
        })?;
        Ok(PantsConfig { build_root, config })
    }
}
