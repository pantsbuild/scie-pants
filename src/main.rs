use std::path::{Path, PathBuf};

use log::info;
use logging_timer::{time, timer, Level};
use serde::Deserialize;

#[derive(Deserialize)]
struct Global {
    pants_version: String,
}

fn default_setup_cache() -> Result<PathBuf, String> {
    if let Some(cache_dir) = dirs::cache_dir() {
        Ok(cache_dir.join("pants").join("setup"))
    } else if let Some(home_dir) = dirs::home_dir() {
        Ok(home_dir.join(".pants-setup"))
    } else {
        Err(
            "Failed to determine a reasonable default cache directory for Pants setup and failed \
            to find a fall back user home directory to establish ~/.pants-setup"
                .to_string(),
        )
    }
}

#[derive(Default, Deserialize)]
struct Setup {
    cache: Option<PathBuf>,
}

#[derive(Deserialize)]
struct Config {
    #[serde(rename = "GLOBAL")]
    global: Global,
    #[serde(default)]
    setup: Setup,
}

struct PantsConfig {
    build_root: PathBuf,
    config: Config,
}

impl PantsConfig {
    #[time("debug")]
    fn find(start_dir: Option<PathBuf>) -> Result<PantsConfig, String> {
        let start_search = if let Some(cwd) = start_dir {
            cwd
        } else {
            std::env::current_dir().map_err(|e| format!("{e}"))?
        };

        let mut cwd = start_search.as_path();
        loop {
            let config_path = cwd.join("pants.toml");
            if config_path.is_file() {
                return Self::parse(cwd, config_path.as_path());
            }
            cwd = cwd.parent().ok_or_else(|| {
                format!(
                    "Failed to find pants.toml starting at {start_search}",
                    start_search = start_search.display()
                )
            })?;
        }
    }

    #[time("debug")]
    fn parse(build_root: &Path, config: &Path) -> Result<PantsConfig, String> {
        let contents = std::fs::read(config).map_err(|e| {
            format!(
                "Failed to read Pants config from {path}: {e}",
                path = config.display()
            )
        })?;
        let config: Config = toml::from_slice(contents.as_slice()).map_err(|e| {
            format!(
                "Failed to parse Pants config from {path}: {e}",
                path = config.display()
            )
        })?;
        Ok(PantsConfig {
            build_root: build_root.to_path_buf(),
            config,
        })
    }

    fn setup_cache(&mut self) -> Result<PathBuf, String> {
        if let Some(setup_cache) = self.config.setup.cache.as_ref() {
            return Ok(setup_cache.clone());
        }

        let default_setup_cache = default_setup_cache()?;
        self.config.setup.cache = Some(default_setup_cache.clone());
        Ok(default_setup_cache)
    }
}

fn main() -> Result<(), String> {
    env_logger::init();
    let _timer = timer!(Level::Debug; "MAIN");

    let mut pants_config = PantsConfig::find(None)?;
    let setup_cache = pants_config.setup_cache()?;
    info!(
        "Found Pants build root at {build_root} and setup cache at {setup_cache}",
        build_root = pants_config.build_root.display(),
        setup_cache = setup_cache.display()
    );
    info!(
        "The required Pants version is {pants_version}",
        pants_version = pants_config.config.global.pants_version
    );
    Ok(())
}
