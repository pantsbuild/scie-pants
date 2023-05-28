# package

A psuedo-crate that serves as the `scie-pants` build tool.

## Usage

Up-to-date usage can be had by passing `--help` like so:
```
$ cargo run -p package -- --help
    Finished dev [unoptimized + debuginfo] target(s) in 0.02s
     Running `target/debug/package --help`
Packages the scie-pants binary.

Usage: package [OPTIONS] <COMMAND>

Commands:
  tools       Builds the `tools.pex` used by the scie-pants scie to perform Pants installs
  scie-pants  Builds the `scie-pants` Rust binary
  scie        Builds the `scie-pants` scie
  test        Builds the `scie-pants` scie and runs it through a series of integration tests
  help        Print this message or the help of the given subcommand(s)

Options:
      --target <TARGET>      Override the default --target for this platform.
      --science <SCIENCE>    Instead of using the released v0.1.2 science, package science from the science project repo at this directory.
      --update-lock          Refresh the tools lock before building the tools.pex
      --dest-dir <DEST_DIR>  The destination directory for the chosen binary and its checksum file. [default: dist]
  -h, --help                 Print help
  -V, --version              Print version
```

In the course of development you'll probably only be interested in two invocations:
+ `cargo run -p package -- scie`:
  The `scie` subcommand builds the `scie-pants` binary and deposits it in `dist/` for
  experimentation.
+ `cargo run -p package -- test`:
  The `test` subcommand both builds the `scie-pants` binary and runs it through a series of
  integration tests.

## Goals

The primary goal of the package crate as build system is to support development of the `scie-pants`
binary with an install of Rust as the ~only requirement (CMake is currently needed as well). This
necessitates dogfooding the same scie mechanism the final `scie-pants` binary uses in production in
order to bootstrap a [Python tool chain](pbt.lift.json). This is used build the [`tools.pex`](
../tools/README.md) embedded in the final `scie-pants` for use in all the slow-path / high-logic
steps like Pants configuration, Pants installation and self-update.

## Structure

The package crate, in the test flow, performs the following build steps from its [main entry point](
src/main.rs):
1. A [`ptex` binary](https://github.com/a-scie/ptex) is built via `cargo install`. This bootstraps
   the ability to fetch further requirements.
2. The current production pins of `ptex` and `scie-jump` are fetched and checksum-verified.
3. A [`pbt`](pbt.lift.json) scie binary is built to facilitate running Python, Pip and Pex tools.
4. The [tools.pex](../tools) is built.
5. The `scie-pants` scie binary is built.
6. The `scie-pants` binary is used to run Pants against the Python tools codebase and then to run a
   series of integration tests exercising the ability to install different Pants vintages and
   configure new Pants projects.
