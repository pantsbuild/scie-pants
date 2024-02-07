# Contributing

The `scie-pants` binary is intended to be the next generation Pants launcher.

Like the current `./pants` bash  script launcher, it determines the required Pants version for the
current Pants-using project and launches it, installing that Pants version if needed.

Unlike the current `./pants` bash script, the `scie-pants` binary:
+ Provides a guaranteed CPython 3.8+ interpreter appropriate to the Pants version being installed.
+ Is intended for use globally as a binary installed on the `PATH`.

When invoked, the `scie-pants` binary searches from the `CWD` of its invocation up through ancestor
directories to find the `pants.toml` signalling a Pants-using project. It then parses `pants.toml`
to discover the Pants version required and launches that Pants version. Since the `scie-pants`
launcher is a native binary, this process is very fast, typically ~one millisecond, in the hot path
where the needed Pants version is already installed.

The slow path, where a new Pants version needs to be installed or Pants bootstrap utility functions
are invoked, is not performance sensitive and can benefit from the Python 3.8+ interpreter the
`scie-pants` launcher guarantees. As such, the Pants installer, bootstrap tools and more are written
in Python 3 and embedded in the `scie-pants` scie as a single `tools.pex` [conscript busy-box](
https://pypi.org/project/conscript/) with multiple console script entry points. This allows for the
core logic of the slow path to be complex but manageable with type-checking, unit testing and all
the power Python generally provides.

Any help pushing forward the `scie-pants` launcher and its supporting Python tools is very welcome.
Thank you in advance for your time and effort.

## Development Environment

You'll need just a few tools to hack on scie-pants:
+ If you're new to Rust you'll need to ensure you have its toolchain available to you. You might
  start with [`rustup`](https://rustup.rs/).
+ Currently, bootstrapping builds [ptex](https://github.com/a-scie/ptex) to use in fetching
  additional build dependencies. Although `ptex` is a Rust project as well, it requires [CMake](
  https://cmake.org/) for one of its sys crates. CMake is generally available via your operating
  system's package manager or Homebrew, for example, if you're on Mac.
+ The [`scie-jump`](https://github.com/a-scie/jump) launcher plays a critical role in the structure
  and operation of `scie-pants`. Although the [package](package/src/main.rs) build process takes
  care of obtaining the `scie-jump` binary itself, you should familiarize yourself with its
  [packaging format](https://github.com/a-scie/jump/blob/main/docs/packaging.md), which both the
  [build process itself](package/pbt.lift.json) and the final [`scie-pants` binary](
  package/scie-pants.lift.json) use.

## Development Cycle

You might want to open a [discussion](https://github.com/pantsbuild/scie-pants/discussions) or
[issue](https://github.com/pantsbuild/scie-pants/issues) to vet your idea first. It can often save
overall effort and lead to a better end result.

The code is run through the ~standard `cargo` gamut. Before sending off changes you should have:
+ Formatted the code (requires Rust nightly): `cargo +nightly fmt --all`
+ Linted the code: `cargo clippy --all`
+ Tested the code: `cargo test --all`

Additionally, you can run any existing integration tests with `cargo run -p package -- test`. This
packages the `scie-pants` scie and then uses it to launch Pants which formats, lints, checks, tests
and re-packages the scie-pants [tools](tools) Python support code.

You can also just package the `scie-pants` scie binary via `cargo run -p package -- scie`. That will
build the `scie-pants` binary for the current machine to the `dist/` directory by default (run
`cargo run -p package -- --help` to find out more options). Two files will be produced there:
1. The `scie-pants` binary: `scie-pants-<os>-<arch>(.<ext>)`
2. The `scie-pants` fingerprint file: `scie-pants-<os>-<arch>(.<ext>).sha256`

You can then run `dist/scie-pants-<os>-<arch>(.<ext>) <pants goals>` to run Pants against the tools
code when iterating on it.

When you're ready to get additional eyes on your changes, submit a [pull request](
https://github.com/pantsbuild/scie-pants/pulls).

## Guiding Principles

There are just a few guiding principles to keep in mind as alluded to above:
+ The `scie-pants` hot path should be fast: It currently launches Pants in ~one millisecond and that
  should hold steady.
+ The `scie-pants` binary should be relatively small: It's currently ~8MB on Linux x86_64 (the
  largest binary). It would be nice to not grow much larger than 10MB.
+ The `scie-pants` binary should be extremely stable: It's intended that there is only ever a single
  main line of development from which releases are cut and the built-in self-update capability is
  used to upgrade with.

## Dependencies

The `scie-pants` project have a few dependencies, and in order to ease maintenance of keeping them
up-to-date, this section details which these are and how to upgrade them.

* The `lift` tool; it is used to build the `scie-pants` binary using a "lift manifest" file.

* The `pants` tool; it is used to manage the Python part of the project.

* The `pex` tool; it is used to package Python packages into executable archives.

* The `ptex` library; it is used to lazyily download required resources in a scie, during runtime.

* Python Build Standalone (PBS) information are included in the scie binary and used when installing
  a Python environment.

* The rust dependencies in `Cargo.toml` are managed as usual for any rust project.

* The `scie_jump` library; it is the core of the scie, with features both to build and launch scie
  binaries, used by `lift`.

### Upgrading `lift`

The `lift` version is defined by the [`SCIENCE_TAG` in package/src/main.rs](package/src/main.rs).

Releases for `lift`: https://github.com/a-scie/lift/releases

### Upgrading `pants`

The `pants` version is defined in [`pants.toml`](pants.toml).

Releases for `pants`: https://github.com/pantsbuild/pants/releases

### Upgrading `pex`

The `pex` tool is used in a couple of places:

* [package/pbt.toml](package/pbt.toml): Update the `pex` details for the entry in `[[lift.files]]`.

* [tools/lock.json](tools/lock.json): Regenerate this lockfile by running:
  `cargo run -p package -- --update-lock`

Releases for `pex`: https://github.com/pantsbuild/pex/releases

Hint: To get the size and hash values, this one-liner is useful:

    curl -L $URL | tee >(wc -c) >(shasum -a 256) >/dev/null

### Upgrading `ptex`

The `ptex` version is defined in [package/pbt.toml](package/pbt.toml) under the `[lift.ptex]` key.

There is also a reference to `ptex` version for bootstrap purposes in [`BOOTSTRAP_PTEX_TAG`](package/src/utils/build.rs).

Releases for `ptex`: https://github.com/a-scie/ptex/releases

### Upgrading `PBS`

The versions of Python Build Standalone to support running `pants` with is defined in
[package/scie-pants.toml](package/scie-pants.toml) under the `[[lift.interpreters]]` group. Make
sure to also update the `members` for the `[[lift.interpreter_groups]]` entry `id=cpython` if
adding/removing interpreters.

Releases for `PBS`: https://github.com/indygreg/python-build-standalone/releases

### Upgrading rust dependencies

This is done by `dependabot` using Pull Requests.

### Upgrading `scie-jump`

The `scie-jump` is referenced from lift manifests under the `[lift.scie_jump]` key:

* [package/pbt.toml](package/pbt.toml)
* [package/scie-pants.toml](package/scie-pants.toml)

Releases of `scie-jump`: https://github.com/a-scie/jump/releases
