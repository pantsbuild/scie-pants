# Release Notes

## 0.9.0

This release adds support for scie-pants to bootstrap Pants using a "per-platform" PEX uploaded as
a GitHub release asset. Additionally, the new wheels.pantsbuild.prg/simple wheels index has been wired
up to support Pants releases back to 2.0.0.dev0. Lastly, the selection of the latest version is now
deferred to the GitHub Release with the "latest" tag.

## 0.8.2

This release fixes handling of environment variables when non-utf8 variables are present in the
ambient environment.

## 0.8.1

This release adjusts the `PANTS_SHA` and `PANTS_VERSION` environment variables to be ignored, if
they're set to an empty string. For instance, `PANTS_SHA= PANTS_VERSION= pants ...` will now behave
the same as `pants ...`.

## 0.8.0

This release brings no new features or bug fixes, but it does convert the building of the
`scie-pants` scie to use `science` from the https://github.com/a-scie project. The only observable
difference is a new top-level `science` object in the `scie-pants` lift manifest that contains
build provenance information (see: `SCIE=inspect pants | jq .science`).

## 0.7.2

This release fixes handling of the Pants native client by ensuring it is executable before trying
to run it.

## 0.7.1

Adds support for using the [Pants native client](https://github.com/pantsbuild/pants/pull/11922),
if it has been included in the Pants distribution. Pants releases starting with `2.17.0a0` are
expected to include the native client.

## 0.7.0

This release updates `scie-jump` to 0.11.0 and `ptex` to 0.7.0. The `scie-jump` upgrade brings in
a feature not yet used by `scie-pants`, but that will be used when it transitions from `scie-jump`
to [`science`](https://github.com/a-scie/lift) for its scie building tool in an upcoming release.
The `ptex` upgrade brings in many fixes in the underlying `curl` code.

The `scie-jump` release notes are here: https://github.com/a-scie/jump/releases/tag/v0.11.0
The `ptex` release notes are here: https://github.com/a-scie/ptex/releases/tag/v0.7.0

## 0.6.1

This release fixes `PANTS_SHA` support to properly cache the resulting Pants install.

## 0.6.0

Support the `PANTS_TOML` environment variable for specifying a location other than the project's
build root for Pants configuration file. This was a missed port from the `./pants` script and
behaves the same: setting the `PANTS_TOML` tells `scie-pants` where to find the config file, but you
must also tell Pants by using `PANTS_CONFIG_FILES` as well. With these two environment variables set
and the project's build root demarcated by either a `BUILD_ROOT` or `BUILDROOT` marker file, Pants
project layouts supported by `./pants` should now be fully supported by `scie-pants`. Since
`scie-pants` supports `.env` files you can also populate both env vars there as now to streamline
the setup.

## 0.5.4

Support using pants from sources also from a projects subtree not only the project root.

## 0.5.3

This release fixes `scie-pants` caching of Pants installs. Previously a given version of Pants was
not fully cached and `scie-pants` would do un-necessary network requests when re-using the already
installed Pants version from a project directory different from the initial installation project
directory. Now a given version of Pants is fully cached per-user (really per `SCIE_BASE`, which
defaults to a cache directory under the user's `HOME` dir).

## 0.5.2

This release fixes `scie-pants` to interoperate with `pants run --debug-adapter`. Previously, if
there was no custom `[debugpy] version` configured, `scie-pants` would cause Pants to error by
passing the empty string as the `debugpy` `version` requirement string via `PANTS_DEBUGPY_VERSION=`.

## 0.5.1

This release silences Pip notifications about new Pip versions being available. The Pip used by
scie-pants is for a one-time install of a Pants in a venv and the version of Pip that ships with
the hermetic Python Build Standalone interpreters suffices for this purpose.

## 0.5.0

This release improves `scie-pants` operation with Pants help by ensuring the command line you used
to invoke Pants is accurately reflected in the help information Pants presents back to you.

## 0.4.2

This release fixes `.pants.bootstrap` handling to robustly mimic handling by the `./pants` script.
The `scie-pants` binary now re-execs itself through a bash shell when `.pants.bootstrap` needs to
be sourced.

## 0.4.1

This release supports using a released Pants version in the Pants repo when a Pants version to use
is defined, treating it as any other project that use Pants as build system.

## 0.4.0

This release supports use of the `scie-pants` binary in the Pants repo being defaulted to
`PANTS_SOURCE=. pants` behavior; i.e.: If you run `pants` in the Pants repo, it will do what you
probably expect: not run Pants from a released version (since the Pants repo specifies none), not
prompt you to set `pants_version` (because that's almost surely not what you want), but run Pants
from the local repo sources.

## 0.3.2

This release fixes the Pants from sources feature added in 0.3.0 to forward command line arguments
to the Pants run from sources correctly. Previously the argument list passed was doubled.

## 0.3.0

This release adds support for running Pants from a local Pants clone. This is useful for testing out
unreleased Pants changes.

This feature used to be provided by a bespoke `pants_from_sources` script copied around to various
repositories; an example of which is [here](https://github.com/pantsbuild/example-python/blob/1b38d08821865e3756024950bc000bdbd0161b95/pants_from_sources).

There are two ways to activate this mode:

1. Execute `pants` with the `PANTS_SOURCE` environment variable set as the path to the Pants repo
   whose Pants code you'd like to run against your repo.
2. Copy, hardlink or symlink your `pants` binary to `pants_from_sources` and execute that.

The first activation method is new. The second mode follows the bespoke `./pants_from_sources`
conventions and assumes `PANTS_SOURCE=../pants`. You can override that by setting the`PANTS_SOURCE`
env var as in the first activation method.

## 0.2.2

This release fixes the scie-pants scie to not expose the interpreter used to run a Pants
installation on the PATH. People using Pants for Python projects will need to supply their own
local Python interpreter for Python goal Processes to use, just like they always have had to.

## 0.2.1

This release fixes un-warranted warnings processing some `.pants.bootstrap` files.

## 0.2.0

This release brings support for loading environment variables into Pants (and `scie-pants`)
environment via the `.env` file convention.

## 0.1.11

This release fixes `SCIE_BOOT=update ./scie-pants`; i.e.: updating `scie-pants` when invoking
`scie-pants` vis a relative path. It also fixes `scie-pants` to work when on the `PATH` as `pants`
in any repo that already contains the `./pants` bash script.

## 0.1.10

This release folds [one step setup](https://github.com/pantsbuild/setup/blob/gh-pages/one_step_setup.sh)
functionality into `scie-pants`.

## 0.1.9

This release fixes a bug using `SCIE_BOOT=update scie-pants` to have
`scie-pants` update itself to the latest stable release. Previously, it
would always update to itself if there was no greater stable version
released. Now, it properly short-circuits and informs that there is no
newer version available.

## 0.1.8

The 1st public release of the project.
