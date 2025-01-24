// Copyright 2023 Pants project contributors.
// Licensed under the Apache License, Version 2.0 (see LICENSE).

use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;
use termcolor::WriteColor;

use crate::build_step;
use crate::utils::build::{BuildContext, Science};
use crate::utils::exe::execute;
use crate::utils::fs::{base_name, copy, ensure_directory, hardlink, path_as_str};

pub(crate) fn build_tools_pex(
    build_context: &BuildContext,
    science: &Science,
    update_lock: bool,
    dest_dir: &Path,
) -> Result<PathBuf> {
    build_step!("Executing science build of the `pbt` helper binary");
    let pbt_package_dir = build_context.cargo_output_root.join("pbt");
    ensure_directory(&pbt_package_dir, true)?;

    let pbt_exe = pbt_package_dir
        .join("pbt")
        .with_extension(env::consts::EXE_EXTENSION);
    let pbt_manifest = build_context.package_crate_root.join("pbt.toml");
    let pbt_manifest_dst = pbt_package_dir.join("lift.toml");
    hardlink(&pbt_manifest, &pbt_manifest_dst)?;

    execute(
        build_context.apply_github_api_bearer_token_if_available(
            science
                .command()
                .args(["lift", "build"])
                .current_dir(&pbt_package_dir),
            "SCIENCE_AUTH_GITHUB_COM_BEARER",
        ),
    )?;

    let tools_path = build_context.workspace_root.join("tools");
    let lock_path = tools_path.join("lock.json");
    let lock = path_as_str(&lock_path)?;
    let requirements_path = tools_path.join("requirements.txt");
    let requirements = path_as_str(&requirements_path)?;
    let test_requirements_path = tools_path.join("test-requirements.txt");
    let test_requirements = path_as_str(&test_requirements_path)?;
    let interpreter_constraints = ["--interpreter-constraint", "CPython>=3.8,<3.12"];

    if update_lock {
        build_step!("Updating the scie_jump tools lock file");
        execute(
            Command::new(&pbt_exe).args(
                [
                    "pex3",
                    "lock",
                    "create",
                    "--style",
                    "universal",
                    "--pip-version",
                    "22.3",
                    "--resolver-version",
                    "pip-2020-resolver",
                    "--no-build",
                    "-o",
                    lock,
                    "--indent",
                    "2",
                    "-r",
                    requirements,
                    "-r",
                    test_requirements,
                ]
                .iter()
                .chain(interpreter_constraints.iter()),
            ),
        )?;
    }

    build_step!("Building the scie_pants `tools.pex`");
    let tools_src_path = tools_path.join("src");
    let tools_src = path_as_str(&tools_src_path)?;
    let tools_pex_path = build_context.cargo_output_root.join("tools.pex");
    let tools_pex = path_as_str(&tools_pex_path)?;
    execute(
        Command::new(&pbt_exe).args(
            [
                "pex",
                "--disable-cache",
                "--no-emit-warnings",
                "--lock",
                lock,
                "-r",
                requirements,
                "-c",
                "conscript",
                "-o",
                tools_pex,
                "--venv",
                "prepend",
                "-D",
                tools_src,
            ]
            .iter()
            .chain(interpreter_constraints.iter()),
        ),
    )?;

    let tools_pex_dest = dest_dir.join(base_name(&tools_pex_path)?);
    ensure_directory(dest_dir, false)?;
    copy(&tools_pex_path, &tools_pex_dest)?;
    Ok(tools_pex_dest)
}
