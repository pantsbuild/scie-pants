# Release Notes

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
