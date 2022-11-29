use log::info;
use logging_timer::{timer, Level};

use crate::config::PantsConfig;

mod config {
    use std::path::{Path, PathBuf};

    use logging_timer::time;
    use serde::Deserialize;

    #[derive(Deserialize)]
    pub(crate) struct Global {
        pub pants_version: String,
    }

    #[derive(Default, Deserialize)]
    pub(crate) struct Setup {
        cache: Option<PathBuf>,
    }

    impl Setup {
        pub(crate) fn cache(&mut self) -> Result<PathBuf, String> {
            if let Some(setup_cache) = self.cache.as_ref() {
                return Ok(setup_cache.clone());
            }

            let default_cache = if let Some(cache_dir) = dirs::cache_dir() {
                cache_dir.join("pants").join("setup")
            } else if let Some(home_dir) = dirs::home_dir() {
                home_dir.join(".pants-setup")
            } else {
                return Err(
                    "Failed to determine a reasonable default cache directory for Pants setup and \
                failed to find a fall back user home directory to establish \
                ~/.pants-setup"
                        .to_string(),
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
        pub(crate) build_root: PathBuf,
        pub(crate) config: Config,
    }

    impl PantsConfig {
        #[time("debug")]
        pub(crate) fn find(start_dir: Option<PathBuf>) -> Result<PantsConfig, String> {
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
    }
}

fn main() -> Result<(), String> {
    env_logger::init();
    let _timer = timer!(Level::Debug; "MAIN");

    let mut pants_config = PantsConfig::find(None)?;
    let setup_cache = pants_config.config.setup.cache()?;
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
