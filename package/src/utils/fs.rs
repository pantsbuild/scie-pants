// Copyright 2023 Pants project contributors.
// Licensed under the Apache License, Version 2.0 (see LICENSE).

use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use log::{info, warn};
use tempfile::TempDir;

pub(crate) fn path_as_str(path: &Path) -> Result<&str> {
    path.to_str()
        .with_context(|| format!("Failed to convert path {path:?} into a UTF-8 str."))
}

pub(crate) fn hardlink(src: &Path, dst: &Path) -> Result<()> {
    info!(
        "Hard linking {src} -> {dst}",
        src = src.display(),
        dst = dst.display()
    );
    std::fs::hard_link(src, dst).with_context(|| {
        format!(
            "Failed to hard link {src} -> {dst}",
            src = src.display(),
            dst = dst.display()
        )
    })
}

pub(crate) fn softlink(src: &Path, dst: &Path) -> Result<()> {
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    #[cfg(windows)]
    use std::os::windows::fs::symlink_file as symlink;

    info!(
        "Soft linking {src} -> {dst}",
        src = src.display(),
        dst = dst.display()
    );
    symlink(src, dst).with_context(|| {
        format!(
            "Failed to soft link {src} -> {dst}",
            src = src.display(),
            dst = dst.display()
        )
    })
}

pub(crate) fn rename(src: &Path, dst: &Path) -> Result<()> {
    info!(
        "Renaming {src} -> {dst}",
        src = src.display(),
        dst = dst.display()
    );
    std::fs::rename(src, dst).with_context(|| {
        format!(
            "Failed to rename {src} -> {dst}",
            src = src.display(),
            dst = dst.display()
        )
    })
}

pub(crate) fn copy(src: &Path, dst: &Path) -> Result<()> {
    info!(
        "Copying {src} -> {dst}",
        src = src.display(),
        dst = dst.display()
    );
    std::fs::copy(src, dst)
        .with_context(|| {
            format!(
                "Failed to copy {src} -> {dst}",
                src = src.display(),
                dst = dst.display()
            )
        })
        .map(|_| ())
}

pub(crate) fn remove_dir(path: &Path) -> Result<()> {
    if path.exists() {
        std::fs::remove_dir_all(path).with_context(|| {
            format!(
                "Failed to remove directory at {path}",
                path = path.display()
            )
        })
    } else {
        Ok(())
    }
}

pub(crate) fn ensure_directory(path: &Path, clean: bool) -> Result<()> {
    if clean {
        if let Err(e) = remove_dir(path) {
            warn!(
                "Failed to clean directory at {path}: {e}",
                path = path.display()
            )
        }
    }
    std::fs::create_dir_all(path).with_context(|| {
        format!(
            "Failed to create directory at {path}",
            path = path.display()
        )
    })
}

pub(crate) fn create_tempdir() -> Result<TempDir> {
    tempfile::tempdir().context("Failed to create a new temporary directory")
}

pub(crate) fn touch(path: &Path) -> Result<()> {
    write_file(path, true, [])
}

pub(crate) fn write_file<C: AsRef<[u8]>>(path: &Path, append: bool, content: C) -> Result<()> {
    if let Some(parent) = path.parent() {
        ensure_directory(parent, false)?;
    }
    let mut fd = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .open(path)
        .with_context(|| format!("Failed to open {path}", path = path.display()))?;
    fd.write_all(content.as_ref())
        .with_context(|| format!("Failed to touch {path}", path = path.display()))
}

pub(crate) fn canonicalize(path: &Path) -> Result<PathBuf> {
    path.canonicalize().with_context(|| {
        format!(
            "Failed to canonicalize {path} to an absolute, resolved path",
            path = path.display()
        )
    })
}

pub(crate) fn base_name(path: &Path) -> Result<&str> {
    path.file_name()
        .with_context(|| format!("Failed to determine the basename of {path:?}"))?
        .to_str()
        .with_context(|| format!("Failed to interpret the basename of {path:?} as a UTF-8 string"))
}

pub(crate) fn dev_cache_dir() -> Result<PathBuf> {
    if let Ok(cache_dir) = env::var("SCIE_PANTS_DEV_CACHE") {
        let cache_path = PathBuf::from(cache_dir);
        ensure_directory(&cache_path, false)?;
        return cache_path.canonicalize().with_context(|| {
            format!(
                "Failed to resolve the absolute path of SCIE_PANTS_DEV_CACHE={cache_dir}",
                cache_dir = cache_path.display()
            )
        });
    }

    let cache_dir = dirs::cache_dir()
        .context("Failed to look up user cache dir for caching scie project downloads")?
        .join("scie-pants")
        .join("dev");
    ensure_directory(&cache_dir, false)?;
    Ok(cache_dir)
}
