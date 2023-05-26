use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;
use termcolor::WriteColor;

use crate::utils::build::{BuildContext, SkinnyScieTools};
use crate::utils::exe::{binary_full_name, execute};
use crate::utils::fs::{ensure_directory, path_as_str};
use crate::{build_step, BINARY};

pub(crate) struct SciePantsBuild {
    pub(crate) exe: PathBuf,
    pub(crate) sha256: PathBuf,
}

pub(crate) fn build_scie_pants_scie(
    build_context: &BuildContext,
    skinny_scie_tools: &SkinnyScieTools,
    scie_pants_exe: &Path,
    tools_pex_file: &Path,
) -> Result<SciePantsBuild> {
    build_step!("Building the `scie-pants` scie");

    let scie_pants_package_dir = build_context.cargo_output_root.join("scie-pants");
    ensure_directory(&scie_pants_package_dir, true)?;

    let scie_pants_manifest = build_context
        .package_crate_root
        .join("scie-pants.toml")
        .strip_prefix(&build_context.workspace_root)?
        .to_owned();

    // N.B.: We name the scie-pants binary scie-pants.bin since the scie itself is named scie-pants
    // which would conflict when packaging.
    execute(
        Command::new(&skinny_scie_tools.science)
            .args([
                "lift",
                "--include-provenance",
                "--file",
                &format!(
                    "scie-pants.bin={scie_pants_exe}",
                    scie_pants_exe = path_as_str(scie_pants_exe)?
                ),
                "--file",
                &format!(
                    "tools.pex={tools_pex}",
                    tools_pex = path_as_str(tools_pex_file)?
                ),
                "build",
                "--dest-dir",
                path_as_str(&scie_pants_package_dir)?,
                "--use-platform-suffix",
                "--hash",
                "sha256",
                path_as_str(&scie_pants_manifest)?,
            ])
            .current_dir(&build_context.workspace_root),
    )?;
    let exe_full_name = binary_full_name(BINARY);
    Ok(SciePantsBuild {
        exe: scie_pants_package_dir.join(exe_full_name.clone()),
        sha256: scie_pants_package_dir.join(format!("{exe_full_name}.sha256")),
    })
}
