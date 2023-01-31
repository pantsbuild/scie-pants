// Copyright 2022 Pants project contributors.
// Licensed under the Apache License, Version 2.0 (see LICENSE).

use std::path::Path;

use anyhow::{Context, Result};
use logging_timer::time;
use serde::Deserialize;

use crate::build_root::BuildRoot;

#[derive(Default, Deserialize)]
pub(crate) struct Global {
    #[serde(default)]
    pub(crate) pants_version: Option<String>,
}

#[derive(Default, Deserialize)]
pub(crate) struct DebugPy {
    pub(crate) version: Option<String>,
}

#[derive(Default, Deserialize)]
pub(crate) struct Default {
    pub(crate) delegate_bootstrap: Option<bool>,
}

#[derive(Deserialize)]
pub(crate) struct Config {
    #[serde(default, rename = "GLOBAL")]
    pub(crate) global: Global,
    #[serde(default)]
    pub(crate) debugpy: DebugPy,
    #[serde(default, rename = "DEFAULT")]
    pub(crate) default: Default,
}

pub(crate) struct PantsConfig {
    build_root: BuildRoot,
    pub(crate) config: Config,
}

impl PantsConfig {
    pub(crate) fn package_version(&self) -> Option<String> {
        self.config.global.pants_version.clone()
    }

    pub(crate) fn build_root(&self) -> &Path {
        self.build_root.as_path()
    }

    pub(crate) fn debugpy_version(&self) -> Option<String> {
        self.config.debugpy.version.clone()
    }

    pub(crate) fn delegate_bootstrap(&self) -> bool {
        self.config.default.delegate_bootstrap.unwrap_or_default()
    }
}

impl PantsConfig {
    #[time("debug", "PantsConfig::{}")]
    pub(crate) fn parse(build_root: BuildRoot) -> Result<PantsConfig> {
        let pants_config = build_root.join("pants.toml");
        let contents = std::fs::read_to_string(&pants_config).with_context(|| {
            format!(
                "Failed to read Pants config from {path}",
                path = pants_config.display()
            )
        })?;
        let config: Config = toml::from_str(&contents).with_context(|| {
            format!(
                "Failed to parse Pants config from {path}",
                path = pants_config.display()
            )
        })?;
        Ok(PantsConfig { build_root, config })
    }
}
