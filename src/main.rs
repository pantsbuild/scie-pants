use std::env;

use log::{debug, info};
use logging_timer::{timer, Level};

use crate::config::{BuildRoot, PantsConfig};

mod config {
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::process::Command;

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

    pub(crate) struct BuildRoot {
        path: PathBuf,
        config: PathBuf,
    }

    impl BuildRoot {
        #[time("debug")]
        pub(crate) fn find(start_dir: Option<PathBuf>) -> Result<BuildRoot, String> {
            let start_search = if let Some(cwd) = start_dir {
                cwd
            } else {
                std::env::current_dir().map_err(|e| format!("{e}"))?
            };

            let mut cwd = start_search.as_path();
            loop {
                let config_path = cwd.join("pants.toml");
                if config_path.is_file() {
                    return Ok(BuildRoot {
                        path: cwd.to_path_buf(),
                        config: config_path,
                    });
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
        pub(crate) fn load_pants_bootstrap(
            &self,
        ) -> Result<Option<Vec<(OsString, OsString)>>, String> {
            let pants_bootstrap = self.path.join(".pants.bootstrap");
            if !pants_bootstrap.is_file() {
                return Ok(None);
            }
            let capture = tempfile::NamedTempFile::new().map_err(|e| {
                format!("Failed to setup pants bootstrap capture temporary file: {e}")
            })?;
            let output = Command::new("bash")
                .args([
                    "-euo",
                    "pipefail",
                    "-c",
                    format!(
                        r#"set >&2; source "{pants_bootstrap}" >"{capture}" 2>&1; set"#,
                        pants_bootstrap = pants_bootstrap.display(),
                        capture = capture.path().display(),
                    )
                    .as_str(),
                ])
                .output()
                .map_err(|e| {
                    format!(
                        "Failed to spawn a bash shell to source {pants_bootstrap}: {e}",
                        pants_bootstrap = pants_bootstrap.display()
                    )
                })?;
            if !output.status.success() {
                return Err(format!(
                    "Failed to source the {pants_bootstrap} script in a bash shell. Process \
                        exited with {code:?} and output:\n{output}",
                    pants_bootstrap = pants_bootstrap.display(),
                    code = output.status.code(),
                    output = std::fs::read_to_string(capture.path()).map_err(|e| format!(
                        "Failed to read output of command to source {pants_bootstrap}: {e}",
                        pants_bootstrap = pants_bootstrap.display()
                    ))?
                ));
            }
            let original_vars = String::from_utf8(output.stderr)
                .map_err(|e| format!("Failed to decode baseline bash environment: {e}"))?;
            let mut original = HashMap::new();
            for line in original_vars.lines() {
                match line.splitn(2, '=').collect::<Vec<_>>()[..] {
                    [key, value] => {
                        original.insert(
                            OsString::from(key.to_string()),
                            OsString::from(value.to_string()),
                        );
                    }
                    _ => eprintln!("Could not interpret env entry {line}. Skipping."),
                }
            }

            let new_vars = String::from_utf8(output.stdout).map_err(|e| {
                format!(
                    "Failed to decode environment modifications of {pants_bootstrap}: {e}",
                    pants_bootstrap = pants_bootstrap.display()
                )
            })?;
            let mut env = vec![];
            for line in new_vars.lines() {
                match line.splitn(2, '=').collect::<Vec<_>>()[..] {
                    [key, value] => {
                        if ["BASH_ARGC", "PIPESTATUS", "_"].contains(&key) {
                            // These are set just by sourcing an empty file and they are not
                            // reasonable env vars for a user of .pants.bootstrap to be trying to
                            // set to influence Pants behavior; so we elide.
                            continue;
                        }
                        let key_os = OsString::from(key.to_string());
                        let value_os = OsString::from(value.to_string());
                        if let Some(new_value_os) = match original.get(&key_os) {
                            Some(original_os) if original_os != &value_os => Some(value_os),
                            None => Some(value_os),
                            _ => None,
                        } {
                            env.push((key_os, new_value_os));
                        }
                    }
                    _ => eprintln!("Could not interpret env entry {line}. Skipping."),
                }
            }
            Ok(Some(env))
        }
    }

    pub(crate) struct PantsConfig {
        pub(crate) build_root: PathBuf,
        pub(crate) config: Config,
    }

    impl PantsConfig {
        #[time("debug")]
        pub(crate) fn parse(build_root: BuildRoot) -> Result<PantsConfig, String> {
            let contents = std::fs::read(&build_root.config).map_err(|e| {
                format!(
                    "Failed to read Pants config from {path}: {e}",
                    path = build_root.config.display()
                )
            })?;
            let config: Config = toml::from_slice(contents.as_slice()).map_err(|e| {
                format!(
                    "Failed to parse Pants config from {path}: {e}",
                    path = build_root.config.display()
                )
            })?;
            Ok(PantsConfig {
                build_root: build_root.path,
                config,
            })
        }
    }
}

fn main() -> Result<(), String> {
    env_logger::init();
    let _timer = timer!(Level::Debug; "MAIN");

    let build_root = BuildRoot::find(None)?;
    if let Some(env) = build_root.load_pants_bootstrap()? {
        for (key, value) in env {
            if let Some(existing_value) = env::var_os(&key) {
                if value != existing_value {
                    debug!("Replacing {key:?}={existing_value:?} with {value:?}");
                    env::set_var(key, value);
                }
            } else {
                debug!("Setting {key:?}={value:?}");
                env::set_var(key, value);
            }
        }
    }
    // TODO(John Sirois): Maybe load .env?
    let mut pants_config = PantsConfig::parse(build_root)?;
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
