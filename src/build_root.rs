// Copyright 2022 Pants project contributors.
// Licensed under the Apache License, Version 2.0 (see LICENSE).

use std::ops::Deref;
use std::path::PathBuf;

use anyhow::{Context, Result};
use logging_timer::time;

pub(crate) struct BuildRoot(PathBuf);

impl BuildRoot {
    #[time("debug", "BuildRoot::{}")]
    pub(crate) fn find(start_dir: Option<PathBuf>) -> Result<BuildRoot> {
        let start_search = if let Some(cwd) = start_dir {
            cwd
        } else {
            std::env::current_dir()?
        };

        let mut cwd = start_search.as_path();
        loop {
            let config_path = cwd.join("pants.toml");
            if config_path.is_file() {
                return Ok(BuildRoot(cwd.to_path_buf()));
            }
            cwd = cwd.parent().with_context(|| {
                format!(
                    "Failed to find pants.toml starting at {start_search}",
                    start_search = start_search.display()
                )
            })?;
        }
    }
}

impl Deref for BuildRoot {
    type Target = PathBuf;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
