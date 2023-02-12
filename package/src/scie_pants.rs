use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;
use termcolor::WriteColor;

use crate::utils::build::{BuildContext, SkinnyScieTools};
use crate::utils::exe::{binary_full_name, execute};
use crate::utils::fs::{ensure_directory, hardlink, rename};
use crate::{build_step, BINARY};

pub(crate) fn build_scie_pants_scie(
    build_context: &BuildContext,
    skinny_scie_tools: &SkinnyScieTools,
    tools_pex_file: &Path,
) -> Result<PathBuf> {
    build_step!("Building the scie-pants Rust binary.");
    let scie_pants_exe = build_context.build_scie_pants()?;

    build_step!("Building the `scie-pants` scie");

    // Setup the scie-pants boot-pack.
    let scie_pants_package_dir = build_context.cargo_output_root.join("scie-pants");
    ensure_directory(&scie_pants_package_dir, true)?;

    let scie_jump_dst = scie_pants_package_dir.join("scie-jump");
    let ptex_dst = scie_pants_package_dir.join("ptex");
    // N.B.: We name the scie-pants binary scie-pants.bin since the scie itself is named scie-pants
    // which would conflict when packaging.
    let scie_pants_dst = scie_pants_package_dir.join("scie-pants.bin");
    let tools_pex_dst = scie_pants_package_dir.join("tools.pex");
    let scie_pants_manifest = build_context
        .package_crate_root
        .join("scie-pants.lift.json");
    let scie_pants_manifest_dst = scie_pants_package_dir.join("lift.json");
    hardlink(&skinny_scie_tools.scie_jump, &scie_jump_dst)?;
    hardlink(&skinny_scie_tools.ptex, &ptex_dst)?;
    hardlink(&scie_pants_exe, &scie_pants_dst)?;
    hardlink(tools_pex_file, &tools_pex_dst)?;
    hardlink(&scie_pants_manifest, &scie_pants_manifest_dst)?;

    // Run the boot-pack.
    execute(Command::new(&scie_jump_dst).current_dir(&scie_pants_package_dir))?;
    let scie_pants_scie = scie_pants_package_dir
        .join(BINARY)
        .with_extension(env::consts::EXE_EXTENSION);
    let scie_pants_scie_with_platform = scie_pants_package_dir.join(binary_full_name(BINARY));
    rename(&scie_pants_scie, &scie_pants_scie_with_platform)?;
    Ok(scie_pants_scie_with_platform)
}
