# Release Notes

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

This release folds [one step setup](
https://github.com/pantsbuild/setup/blob/gh-pages/one_step_setup.sh)
functionality into `scie-pants`.

## 0.1.9

This release fixes a bug using `SCIE_BOOT=update scie-pants` to have
`scie-pants` update itself to the latest stable release. Previously, it
would always update to itself if there was no greater stable version
released. Now, it properly short-circuits and informs that there is no
newer version available.

## 0.1.8

The 1st public release of the project.
