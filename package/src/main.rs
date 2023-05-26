// Copyright 2022 Pants project contributors.
// Licensed under the Apache License, Version 2.0 (see LICENSE).

mod scie_pants;

#[macro_use]
mod test;

mod tools_pex;

#[macro_use]
mod utils;

use std::fmt::{Display, Formatter};
use std::io::Write;
use std::ops::Deref;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use clap::{arg, command, Parser, Subcommand};
use termcolor::{Color, WriteColor};
use utils::fs;

use crate::scie_pants::{build_scie_pants_scie, SciePantsBuild};
use crate::test::run_integration_tests;
use crate::tools_pex::build_tools_pex;
use crate::utils::build::{check_sha256, fetch_skinny_scie_tools, BuildContext};
use crate::utils::fs::{canonicalize, copy, ensure_directory};

const BINARY: &str = "scie-pants";

const SCIENCE_TAG: &str = "v0.1.1";

#[derive(Clone)]
struct SpecifiedPath(PathBuf);

impl SpecifiedPath {
    fn new(path: &str) -> Self {
        Self::from(path.to_string())
    }
}

impl From<String> for SpecifiedPath {
    fn from(path: String) -> Self {
        SpecifiedPath(PathBuf::from(path))
    }
}

impl Deref for SpecifiedPath {
    type Target = PathBuf;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<Path> for SpecifiedPath {
    fn as_ref(&self) -> &Path {
        self.0.as_path()
    }
}

impl Display for SpecifiedPath {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.display())
    }
}

#[derive(Subcommand)]
enum Commands {
    /// Builds the `tools.pex` used by the scie-pants scie to perform Pants installs.
    Tools,
    /// Builds the `scie-pants` scie.
    Scie {
        #[arg(
            long,
            help = "The location of the pre-built tools.pex to use. By default, the tools.pex is \
            built fresh."
        )]
        tools_pex: Option<PathBuf>,
    },
    /// Builds the `scie-pants` scie and runs it through a series of integration tests.
    Test {
        #[arg(
            long,
            help = "The location of the pre-built tools.pex to use. By default, the tools.pex is \
            built fresh."
        )]
        tools_pex: Option<PathBuf>,
        #[arg(
            long,
            help = "The location of the pre-built scie-pants scie to use. By default, the \
            scie-pants scie is built fresh."
        )]
        scie_pants: Option<PathBuf>,
        #[arg(
            long,
            help = "Only check formatting and lints and fail the tests if these checks fail \
            instead of re-formatting.",
            default_value_t = false
        )]
        check: bool,
        #[arg(
            long,
            help = "Only warn if the Pants built tools.pex doesn't match ours instead of failing \
            the tests.",
            default_value_t = false
        )]
        tools_pex_mismatch_warn: bool,
    },
}

#[derive(Parser)]
#[command(about, version)]
struct Args {
    #[arg(long, help = "Override the default --target for this platform.")]
    target: Option<String>,
    #[arg(
        long,
        help = format!(
            "Instead of using the released {SCIENCE_TAG} science, package science from the science \
            project repo at this directory."
        )
    )]
    science: Option<PathBuf>,
    #[arg(
        long,
        help = "Refresh the tools lock before building the tools.pex",
        default_value_t = false
    )]
    update_lock: bool,
    #[arg(
        long,
        help = "The destination directory for the chosen binary and its checksum file.",
        default_value_t = SpecifiedPath::new("dist")
    )]
    dest_dir: SpecifiedPath,
    #[command(subcommand)]
    command: Commands,
}

fn maybe_build(args: &Args, build_context: &BuildContext) -> Result<Option<SciePantsBuild>> {
    match &args.command {
        Commands::Test {
            tools_pex: Some(tools_pex),
            scie_pants: Some(scie_pants),
            check,
            tools_pex_mismatch_warn,
        } => {
            run_integration_tests(
                &build_context.workspace_root,
                &canonicalize(tools_pex)?,
                &canonicalize(scie_pants)?,
                *check,
                *tools_pex_mismatch_warn,
            )?;
            Ok(None)
        }
        Commands::Test {
            tools_pex: None,
            scie_pants: Some(scie_pants),
            check,
            tools_pex_mismatch_warn,
        } => {
            let skinny_scie_tools = fetch_skinny_scie_tools(build_context)?;
            let tools_pex = build_tools_pex(
                build_context,
                &skinny_scie_tools,
                args.update_lock,
                args.dest_dir.as_path(),
            )?;
            run_integration_tests(
                &build_context.workspace_root,
                &tools_pex,
                &canonicalize(scie_pants)?,
                *check,
                *tools_pex_mismatch_warn,
            )?;
            Ok(None)
        }
        Commands::Test {
            tools_pex: Some(tools_pex),
            scie_pants: None,
            check,
            tools_pex_mismatch_warn,
        } => {
            let skinny_scie_tools = fetch_skinny_scie_tools(build_context)?;
            let scie_pants = build_scie_pants_scie(build_context, &skinny_scie_tools, tools_pex)?;
            run_integration_tests(
                &build_context.workspace_root,
                &canonicalize(tools_pex)?,
                &scie_pants.exe,
                *check,
                *tools_pex_mismatch_warn,
            )?;
            Ok(Some(scie_pants))
        }
        Commands::Test {
            tools_pex: None,
            scie_pants: None,
            check,
            tools_pex_mismatch_warn,
        } => {
            let skinny_scie_tools = fetch_skinny_scie_tools(build_context)?;
            let tools_pex = build_tools_pex(
                build_context,
                &skinny_scie_tools,
                args.update_lock,
                args.dest_dir.as_path(),
            )?;
            let scie_pants = build_scie_pants_scie(build_context, &skinny_scie_tools, &tools_pex)?;
            run_integration_tests(
                &build_context.workspace_root,
                &tools_pex,
                &scie_pants.exe,
                *check,
                *tools_pex_mismatch_warn,
            )?;
            Ok(Some(scie_pants))
        }
        Commands::Scie { tools_pex: None } => {
            let skinny_scie_tools = fetch_skinny_scie_tools(build_context)?;
            let tools_pex = build_tools_pex(
                build_context,
                &skinny_scie_tools,
                args.update_lock,
                args.dest_dir.as_path(),
            )?;
            Ok(Some(build_scie_pants_scie(
                build_context,
                &skinny_scie_tools,
                &tools_pex,
            )?))
        }
        Commands::Scie {
            tools_pex: Some(tools_pex),
        } => {
            let skinny_scie_tools = fetch_skinny_scie_tools(build_context)?;
            Ok(Some(build_scie_pants_scie(
                build_context,
                &skinny_scie_tools,
                tools_pex,
            )?))
        }
        Commands::Tools => {
            let skinny_scie_tools = fetch_skinny_scie_tools(build_context)?;
            build_tools_pex(
                build_context,
                &skinny_scie_tools,
                args.update_lock,
                args.dest_dir.as_path(),
            )?;
            Ok(None)
        }
    }
}

fn main() -> Result<()> {
    pretty_env_logger::init();

    let args = Args::parse();

    let dest_dir = &args.dest_dir;
    if dest_dir.is_file() {
        bail!(
            "The specified dest_dir of {dest_dir} is a file. Not overwriting",
            dest_dir = dest_dir.display()
        );
    }

    let build_context = BuildContext::new(args.target.as_deref(), args.science.as_deref())?;
    if let Some(scie_pants) = maybe_build(&args, &build_context)? {
        ensure_directory(dest_dir, false)?;

        let dest_file_name = fs::base_name(&scie_pants.exe)?;
        let dest_file = dest_dir.join(dest_file_name);
        copy(&scie_pants.exe, &dest_file)?;
        copy(
            &scie_pants.sha256,
            &dest_dir.join(fs::base_name(&scie_pants.sha256)?),
        )?;

        check_sha256(&dest_file)?;

        log!(
            Color::Yellow,
            "Wrote {dest_file_name} to {dest_file}",
            dest_file = dest_file.display()
        );
    }

    Ok(())
}
