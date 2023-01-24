// Copyright 2022 Pants project contributors.
// Licensed under the Apache License, Version 2.0 (see LICENSE).

use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::process::Command;

use anyhow::{bail, Context, Result};
use log::debug;
use logging_timer::time;

use crate::build_root::BuildRoot;

pub(crate) struct PantsBootstrap {
    env: Vec<(OsString, OsString)>,
}

impl PantsBootstrap {
    #[time("debug", "PantsBootstrap::{}")]
    pub(crate) fn load(build_root: &BuildRoot) -> Result<Option<Self>> {
        let pants_bootstrap = build_root.join(".pants.bootstrap");
        if !pants_bootstrap.is_file() {
            return Ok(None);
        }
        let capture = tempfile::NamedTempFile::new()
            .context("Failed to setup pants bootstrap capture temporary file")?;
        let output = Command::new("bash")
            .args([
                "-euo",
                "pipefail",
                "-c",
                // N.B.: The `set` are used to print the current variables. The set -o posix are
                // used to toggle restriction of `set` to printing only variable names and not more
                // exotic items like functions. We ensure our modifications are unset before
                // sourcing the .pants.bootstrap by using a sub-shell so that that script can use
                // full bash-isms as has been the contract established in the original `./pants`
                // script.
                // N.B.: We take care to ensure the bash script here is a single line since the
                // script contents are re-capitulated in the BASH_EXECUTION_STRING env var and a
                // multiline env var will have its trailing lines fail to parse as env vars in most
                // cases.
                [
                    r#"(set -o posix; IFS=$'\0'; set >&2); "#,
                    format!(
                        r#"source "{pants_bootstrap}" >"{capture}" 2>&1; "#,
                        pants_bootstrap = pants_bootstrap.display(),
                        capture = capture.path().display(),
                    ).as_str(),
                    r#"set -o posix; IFS=$'\0'; set"#
                ].join("").as_str(),
            ])
            .output()
            .with_context(|| {
                format!(
                    "Failed to spawn a bash shell to source {pants_bootstrap}",
                    pants_bootstrap = pants_bootstrap.display()
                )
            })?;
        if !output.status.success() {
            bail!(
                "Failed to source the {pants_bootstrap} script in a bash shell. Process \
                    exited with code {code} and output:\n{output}",
                pants_bootstrap = pants_bootstrap.display(),
                code = output
                    .status
                    .code()
                    .map_or_else(|| "<unknown>".to_string(), |code| format!("{code}")),
                output = std::fs::read_to_string(capture.path()).with_context(|| {
                    format!(
                        "Failed to read output of command to source {pants_bootstrap}",
                        pants_bootstrap = pants_bootstrap.display()
                    )
                })?
            )
        }
        let original_vars = String::from_utf8(output.stderr)
            .context("Failed to decode baseline bash environment: {e}")?;
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

        let new_vars = String::from_utf8(output.stdout).with_context(|| {
            format!(
                "Failed to decode environment modifications of {pants_bootstrap}",
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
        Ok(Some(Self { env }))
    }

    #[time("debug", "PantsBootstrap::{}")]
    pub(crate) fn export_env(&self) {
        for (key, value) in &self.env {
            if let Some(ref existing_value) = env::var_os(key) {
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
}
