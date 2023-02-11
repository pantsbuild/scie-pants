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

use clap::{arg, command, Parser, Subcommand};
use proc_exit::{Code, Exit, ExitResult};
use termcolor::{Color, WriteColor};

use crate::scie_pants::build_scie_pants_scie;
use crate::test::run_integration_tests;
use crate::tools_pex::build_tools_pex;
use crate::utils::build::{fetch_skinny_scie_tools, fingerprint, BuildContext};
use crate::utils::fs::{canonicalize, copy, ensure_directory};

const BINARY: &str = "scie-pants";

const PTEX_TAG: &str = "v0.6.0";
const SCIE_JUMP_TAG: &str = "v0.10.0";

fn check_sha256(path: &Path) -> ExitResult {
    let sha256_file = PathBuf::from(format!("{path}.sha256", path = path.display()));
    let contents = std::fs::read_to_string(&sha256_file).map_err(|e| {
        Code::FAILURE.with_message(format!(
            "Failed to read {sha256_file}: {e}",
            sha256_file = sha256_file.display()
        ))
    })?;
    let expected_sha256 = contents.split(' ').next().ok_or_else(|| {
        Code::FAILURE.with_message(format!(
            "Expected {sha256_file} to have a leading hash",
            sha256_file = sha256_file.display()
        ))
    })?;
    assert_eq!(expected_sha256, fingerprint(path)?.as_str());
    Ok(())
}

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
            "Instead of using the released {PTEX_TAG} ptex, package ptex from the ptex project \
            repo at this directory."
        )
    )]
    ptex: Option<PathBuf>,
    #[arg(
        long,
        help = format!(
            "Instead of using the released {SCIE_JUMP_TAG} scie-jump, package the scie-jump from \
            the scie-jump project repo at this directory."
        )
    )]
    scie_jump: Option<PathBuf>,
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

fn maybe_build(args: &Args, build_context: &BuildContext) -> Result<Option<PathBuf>, Exit> {
    match &args.command {
        Commands::Test {
            tools_pex: Some(tools_pex),
            scie_pants: Some(scie_pants),
            tools_pex_mismatch_warn,
        } => {
            run_integration_tests(
                &build_context.workspace_root,
                &canonicalize(tools_pex)?,
                &canonicalize(scie_pants)?,
                *tools_pex_mismatch_warn,
            )?;
            Ok(None)
        }
        Commands::Test {
            tools_pex: None,
            scie_pants: Some(scie_pants),
            tools_pex_mismatch_warn,
        } => {
            let skinny_scie_tools = fetch_skinny_scie_tools(build_context)?;
            let tools_pex = build_tools_pex(build_context, &skinny_scie_tools, args.update_lock)?;
            run_integration_tests(
                &build_context.workspace_root,
                &tools_pex,
                &canonicalize(scie_pants)?,
                *tools_pex_mismatch_warn,
            )?;
            Ok(None)
        }
        Commands::Test {
            tools_pex: Some(tools_pex),
            scie_pants: None,
            tools_pex_mismatch_warn,
        } => {
            let skinny_scie_tools = fetch_skinny_scie_tools(build_context)?;
            let scie_pants = build_scie_pants_scie(build_context, &skinny_scie_tools, tools_pex)?;
            run_integration_tests(
                &build_context.workspace_root,
                &canonicalize(tools_pex)?,
                &scie_pants,
                *tools_pex_mismatch_warn,
            )?;
            Ok(Some(scie_pants))
        }
        Commands::Test {
            tools_pex: None,
            scie_pants: None,
            tools_pex_mismatch_warn,
        } => {
            let skinny_scie_tools = fetch_skinny_scie_tools(build_context)?;
            let tools_pex = build_tools_pex(build_context, &skinny_scie_tools, args.update_lock)?;
            let scie_pants = build_scie_pants_scie(build_context, &skinny_scie_tools, &tools_pex)?;
            run_integration_tests(
                &build_context.workspace_root,
                &tools_pex,
                &scie_pants,
                *tools_pex_mismatch_warn,
            )?;
            Ok(Some(scie_pants))
        }
        Commands::Scie { tools_pex: None } => {
            let skinny_scie_tools = fetch_skinny_scie_tools(build_context)?;
            let tools_pex = build_tools_pex(build_context, &skinny_scie_tools, args.update_lock)?;
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
            Ok(Some(build_tools_pex(
                build_context,
                &skinny_scie_tools,
                args.update_lock,
            )?))
        }
    }
}

fn main() -> ExitResult {
    pretty_env_logger::init();

    let args = Args::parse();

    let dest_dir = &args.dest_dir;
    if dest_dir.is_file() {
        return Err(Code::FAILURE.with_message(format!(
            "The specified dest_dir of {} is a file. Not overwriting",
            dest_dir.display()
        )));
    }

    let build_context = BuildContext::new(
        args.target.as_deref(),
        args.ptex.as_deref(),
        args.scie_jump.as_deref(),
    )?;
    if let Some(output_file) = maybe_build(&args, &build_context)? {
        let dest_file_name = output_file
            .file_name()
            .ok_or_else(|| {
                Code::FAILURE.with_message(format!(
                    "Failed to determine the basename of {output_file:?}"
                ))
            })?
            .to_str()
            .ok_or_else(|| {
                Code::FAILURE.with_message(format!(
                    "Failed to interpret the basename of {output_file:?} as a UTF-8 string"
                ))
            })?;
        let dest_file = dest_dir.join(dest_file_name);
        ensure_directory(dest_dir, false)?;
        copy(&output_file, &dest_file)?;

        let fingerprint_file = dest_file.with_file_name(format!("{dest_file_name}.sha256"));
        let dest_file_digest = fingerprint(&dest_file)?;
        std::fs::write(
            &fingerprint_file,
            format!("{dest_file_digest} *{dest_file_name}\n"),
        )
        .map_err(|e| {
            Code::FAILURE.with_message(format!(
                "Failed to write fingerprint file {fingerprint_file}: {e}",
                fingerprint_file = fingerprint_file.display()
            ))
        })?;
        check_sha256(&dest_file)?;

        log!(
            Color::Yellow,
            "Wrote {dest_file_name} to {dest_file}",
            dest_file = dest_file.display()
        );
    }

    Ok(())
}
