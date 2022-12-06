// Copyright 2022 Pants project contributors.
// Licensed under the Apache License, Version 2.0 (see LICENSE).

use std::fmt::{Display, Formatter};
use std::path::Path;

use anyhow::{Context, Result};
use logging_timer::time;
use pyver::PackageVersion;
use serde::Deserialize;

use crate::build_root::BuildRoot;

#[derive(Debug, Deserialize)]
#[serde(try_from = "String")]
pub struct Version(PackageVersion);

impl TryFrom<String> for Version {
    type Error = anyhow::Error;

    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        PackageVersion::new(value.as_str()).map(Self)
    }
}

impl Display for Version {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Default, Deserialize)]
pub(crate) struct Global {
    #[serde(default)]
    pub(crate) pants_version: Option<Version>,
}

#[derive(Default, Deserialize)]
pub(crate) struct DebugPy {
    pub(crate) version: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct Config {
    #[serde(default, rename = "GLOBAL")]
    pub(crate) global: Global,
    #[serde(default)]
    pub(crate) debugpy: DebugPy,
}

pub(crate) struct PantsConfig {
    build_root: BuildRoot,
    pub(crate) config: Config,
}

impl PantsConfig {
    pub(crate) fn package_version(&self) -> Option<&PackageVersion> {
        self.config.global.pants_version.as_ref().map(|pv| &pv.0)
    }

    pub(crate) fn build_root(&self) -> &Path {
        self.build_root.as_path()
    }

    pub(crate) fn debugpy_version(&self) -> Option<String> {
        self.config.debugpy.version.clone()
    }
}

impl PantsConfig {
    #[time("debug", "PantsConfig::{}")]
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
