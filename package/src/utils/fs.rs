// Copyright 2023 Pants project contributors.
// Licensed under the Apache License, Version 2.0 (see LICENSE).

use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};

use log::{info, warn};
use proc_exit::{Code, Exit, ExitResult};
use tempfile::TempDir;

pub(crate) fn path_as_str(path: &Path) -> Result<&str, Exit> {
    path.to_str().ok_or_else(|| {
        Code::FAILURE.with_message(format!("Failed to convert path {path:?} into a UTF-8 str."))
    })
}

pub(crate) fn hardlink(src: &Path, dst: &Path) -> ExitResult {
    info!(
        "Hard linking {src} -> {dst}",
        src = src.display(),
        dst = dst.display()
    );
    std::fs::hard_link(src, dst).map_err(|e| {
        Code::FAILURE.with_message(format!(
            "Failed to hard link {src} -> {dst}: {e}",
            src = src.display(),
            dst = dst.display()
        ))
    })
}

pub(crate) fn softlink(src: &Path, dst: &Path) -> ExitResult {
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    #[cfg(windows)]
    use std::os::windows::fs::symlink_file as symlink;

    info!(
        "Soft linking {src} -> {dst}",
        src = src.display(),
        dst = dst.display()
    );
    symlink(src, dst).map_err(|e| {
        Code::FAILURE.with_message(format!(
            "Failed to soft link {src} -> {dst}: {e}",
            src = src.display(),
            dst = dst.display()
        ))
    })
}

pub(crate) fn rename(src: &Path, dst: &Path) -> ExitResult {
    info!(
        "Renaming {src} -> {dst}",
        src = src.display(),
        dst = dst.display()
    );
    std::fs::rename(src, dst).map_err(|e| {
        Code::FAILURE.with_message(format!(
            "Failed to rename {src} -> {dst}: {e}",
            src = src.display(),
            dst = dst.display()
        ))
    })
}

pub(crate) fn copy(src: &Path, dst: &Path) -> ExitResult {
    info!(
        "Copying {src} -> {dst}",
        src = src.display(),
        dst = dst.display()
    );
    std::fs::copy(src, dst)
        .map_err(|e| {
            Code::FAILURE.with_message(format!(
                "Failed to copy {src} -> {dst}: {e}",
                src = src.display(),
                dst = dst.display()
            ))
        })
        .map(|_| ())
}

pub(crate) fn remove_dir(path: &Path) -> ExitResult {
    if path.exists() {
        std::fs::remove_dir_all(path).map_err(|e| {
            Code::FAILURE.with_message(format!(
                "Failed to remove directory at {path}: {e}",
                path = path.display()
            ))
        })
    } else {
        Ok(())
    }
}

pub(crate) fn ensure_directory(path: &Path, clean: bool) -> ExitResult {
    if clean {
        if let Err(e) = remove_dir(path) {
            warn!(
                "Failed to clean directory at {path}: {e}",
                path = path.display()
            )
        }
    }
    std::fs::create_dir_all(path).map_err(|e| {
        Code::FAILURE.with_message(format!(
            "Failed to create directory at {path}: {e}",
            path = path.display()
        ))
    })
}

pub(crate) fn create_tempdir() -> Result<TempDir, Exit> {
    tempfile::tempdir().map_err(|e| {
        Code::FAILURE.with_message(format!("Failed to create a new temporary directory: {e}"))
    })
}

pub(crate) fn touch(path: &Path) -> ExitResult {
    write_file(path, true, [])
}

pub(crate) fn write_file<C: AsRef<[u8]>>(path: &Path, append: bool, content: C) -> ExitResult {
    if let Some(parent) = path.parent() {
        ensure_directory(parent, false)?;
    }
    let mut fd = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .open(path)
        .map_err(|e| {
            Code::FAILURE.with_message(format!("Failed to open {path}: {e}", path = path.display()))
        })?;
    fd.write_all(content.as_ref()).map_err(|e| {
        Code::FAILURE.with_message(format!(
            "Failed to touch {path}: {e}",
            path = path.display()
        ))
    })
}

pub(crate) fn canonicalize(path: &Path) -> Result<PathBuf, Exit> {
    path.canonicalize().map_err(|e| {
        Code::FAILURE.with_message(format!(
            "Failed to canonicalize {path} to an absolute, resolved path: {e}",
            path = path.display()
        ))
    })
}

pub(crate) fn dev_cache_dir() -> Result<PathBuf, Exit> {
    if let Ok(cache_dir) = env::var("SCIE_PANTS_DEV_CACHE") {
        let cache_path = PathBuf::from(cache_dir);
        ensure_directory(&cache_path, false)?;
        return cache_path.canonicalize().map_err(|e| {
            Code::FAILURE.with_message(format!(
                "Failed to resolve the absolute path of SCIE_PANTS_DEV_CACHE={cache_dir}: {e}",
                cache_dir = cache_path.display()
            ))
        });
    }

    let cache_dir = dirs::cache_dir()
        .ok_or_else(|| {
            Code::FAILURE.with_message(
                "Failed to look up user cache dir for caching scie project downloads".to_string(),
            )
        })?
        .join("scie-pants")
        .join("dev");
    ensure_directory(&cache_dir, false)?;
    Ok(cache_dir)
}
