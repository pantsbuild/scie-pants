// Copyright 2023 Pants project contributors.
// Licensed under the Apache License, Version 2.0 (see LICENSE).

use std::env;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use anyhow::{Context, Result};
use regex::Regex;
use tempfile::TempDir;
use termcolor::{Color, WriteColor};

use crate::utils::build::fingerprint;
use crate::utils::exe::{execute, execute_with_input, Platform, CURRENT_PLATFORM};
use crate::utils::fs::{
    copy, create_tempdir, ensure_directory, remove_dir, rename, softlink, touch, write_file,
};
use crate::utils::os::{EOL, PATHSEP};
use crate::{build_step, log};

macro_rules! integration_test {
    ($msg:expr $(,)?) => {
        log!(::termcolor::Color::Magenta, ">> {}", format!($msg));
    };
    ($msg:expr $(,)?, $($arg:tt)*) => {
        log!(::termcolor::Color::Magenta, ">> {}", format!($msg, $($arg)*));
    };
}

macro_rules! issue_link {
    ($issue: expr) => {
        issue_link($issue, "pantsbuild/scie-pants")
    };
    ($issue: expr, $repo: expr) => {
        issue_link($issue, $repo)
    };
}

fn issue_link(issue: usize, repo: &str) -> String {
    format!("https://github.com/{repo}/issues/{issue}")
}

fn decode_output(output: Vec<u8>) -> Result<String> {
    String::from_utf8(output).context("Failed to decode Pants output.")
}

/// Returns true if the current platform is a macOS major version that's older than the requested minimums.
///
/// (NB. Running on a non-macOS platform will always return false.)
fn is_macos_thats_too_old(minimum_x86_64: i64, minimum_arm64: i64) -> bool {
    let min_major = match *CURRENT_PLATFORM {
        Platform::MacOSX86_64 => minimum_x86_64,
        Platform::MacOSAarch64 => minimum_arm64,
        _ => return false,
    };

    let version_output = execute(
        Command::new("sw_vers")
            .arg("-productVersion")
            .stdout(Stdio::piped()),
    )
    .unwrap();
    let version_str = decode_output(version_output.stdout).unwrap();

    // for this constrained use case, we can just parse the first element, e.g. 10.14 & 10.15 => 10,
    // 11.0.1 => 11, etc.
    //
    // If the distinction between the 10.x "major" versions ends up mattering, feel free to refactor
    // this to work with the full version string.
    let major: i64 = version_str
        .trim()
        .split('.')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| {
            panic!(
                "Failed to parse macOS version from `sw_vers -productVersion` output: {}",
                version_str
            )
        });
    major < min_major
}

enum ExpectedResult {
    Success,
    Failure,
}

fn assert_stderr_output(
    command: &mut Command,
    expected_messages: Vec<&str>,
    expected_result: ExpectedResult,
) -> (Output, String) {
    command.stderr(Stdio::piped());

    let output = match expected_result {
        ExpectedResult::Success => execute(command).unwrap(),
        ExpectedResult::Failure => {
            let output = command.spawn().unwrap().wait_with_output().unwrap();
            assert!(
                !output.status.success(),
                "Command {:?} unexpectedly succeeded, STDERR: {}",
                command,
                decode_output(output.stderr).unwrap()
            );
            output
        }
    };

    let stderr = decode_output(output.stderr.clone()).unwrap();
    for expected_message in expected_messages {
        assert!(
            stderr.contains(expected_message),
            "STDERR did not contain '{expected_message}':\n{stderr}"
        );
    }
    (output, stderr)
}

pub(crate) fn run_integration_tests(
    workspace_root: &Path,
    tools_pex_path: &Path,
    scie_pants_scie: &Path,
    check: bool,
    tools_pex_mismatch_warn: bool,
) -> Result<()> {
    build_step!("Running smoke tests");
    log!(
        Color::Yellow,
        "Disabling pants rc files for the smoke tests."
    );
    env::set_var("PANTS_PANTSRC", "False");

    // Our `.pants.bootstrap` uses `tput` which requires TERM be set: ensure it is.
    env::set_var("TERM", env::var_os("TERM").unwrap_or_else(|| "dumb".into()));

    // Max Python supported is 3.9 and only Linux x86_64 and macOS aarch64 and x86_64 wheels were
    // released.
    if matches!(
        *CURRENT_PLATFORM,
        Platform::LinuxX86_64 | Platform::MacOSAarch64 | Platform::MacOSX86_64
    ) {
        test_tools(scie_pants_scie, check);
        test_pants_bin_name_handling(scie_pants_scie);
        test_pants_bootstrap_handling(scie_pants_scie);
        test_pants_bootstrap_stdout_silent(scie_pants_scie);
        test_tools_pex_reproducibility(workspace_root, tools_pex_path, tools_pex_mismatch_warn);
        test_pants_bootstrap_tools(scie_pants_scie);

        log!(Color::Yellow, "Turning off pantsd for remaining tests.");
        env::set_var("PANTS_PANTSD", "False");

        test_pants_2_25_using_python_3_11(scie_pants_scie);
        test_python_repos_repos(scie_pants_scie);
        test_initialize_new_pants_project(scie_pants_scie);
        test_set_pants_version(scie_pants_scie);
        test_ignore_empty_pants_version(scie_pants_scie);

        test_pants_from_pex_version(scie_pants_scie);
        test_pants_from_bad_pex_version(scie_pants_scie);

        let clone_root = create_tempdir()?;
        test_use_in_repo_with_pants_script(scie_pants_scie, &clone_root);
        test_dot_env_loading(scie_pants_scie, &clone_root);
        test_dot_env_error(scie_pants_scie);

        let dev_cache_dir = crate::utils::fs::dev_cache_dir()?;
        let clone_dir = dev_cache_dir.join("clones");
        let pants_2_25_0_dev1_clone_dir = clone_dir.join("pants-2.25.0.dev1");
        let venv_dir = dev_cache_dir.join("venvs");
        let pants_2_25_0_dev1_venv_dir = venv_dir.join("pants-2.25.0.dev1");

        test_pants_source_mode(
            scie_pants_scie,
            &clone_dir,
            &pants_2_25_0_dev1_clone_dir,
            &venv_dir,
            &pants_2_25_0_dev1_venv_dir,
        );
        test_pants_from_sources_mode(
            scie_pants_scie,
            &pants_2_25_0_dev1_clone_dir,
            &pants_2_25_0_dev1_venv_dir,
        );
        test_delegate_pants_in_pants_repo(scie_pants_scie, &pants_2_25_0_dev1_clone_dir);
        test_use_pants_release_in_pants_repo(scie_pants_scie, &pants_2_25_0_dev1_clone_dir);

        test_caching_issue_129(scie_pants_scie);
        test_custom_pants_toml_issue_153(scie_pants_scie);
        test_pants_native_client_perms_issue_182(scie_pants_scie);

        #[cfg(unix)]
        test_non_utf8_env_vars_issue_198(scie_pants_scie);

        test_bad_boot_error_text(scie_pants_scie);
        test_pants_bootstrap_urls(scie_pants_scie);
    }

    // Max Python supported is 3.8 and only Linux and macOS x86_64 wheels were released.
    if matches!(
        *CURRENT_PLATFORM,
        Platform::LinuxX86_64 | Platform::MacOSX86_64
    ) {
        test_python38_used_for_old_pants(scie_pants_scie);
    }

    test_self_update(scie_pants_scie);
    test_self_downgrade(scie_pants_scie);

    Ok(())
}

fn test_tools(scie_pants_scie: &Path, check: bool) {
    integration_test!("Linting, testing and packaging the tools codebase");

    let tput_output = |subcommand| {
        let result = execute(Command::new("tput").arg(subcommand).stdout(Stdio::piped()))
            .unwrap()
            .stdout;
        String::from_utf8(result)
            .with_context(|| format!("Failed to decode output of tput {subcommand} as UTF-*"))
            .unwrap()
    };
    let mut command = Command::new(scie_pants_scie);
    if !check {
        command.arg("fmt");
    }
    execute(
        command
            .args([
                "tailor",
                "--check",
                "update-build-files",
                "--check",
                "lint",
                "check",
                "test",
                "package",
                "::",
            ])
            .env("PEX_SCRIPT", "Does not exist!")
            .env("EXPECTED_COLUMNS", tput_output("cols").trim())
            .env("EXPECTED_LINES", tput_output("lines").trim()),
    )
    .unwrap();
}

fn test_pants_bin_name_handling(scie_pants_scie: &Path) {
    integration_test!("Checking PANTS_BIN_NAME handling");
    let check_pants_bin_name_chroot = create_tempdir().unwrap();

    let bin_dir = check_pants_bin_name_chroot.path().join("bin");
    let project_dir = check_pants_bin_name_chroot.path().join("project");
    let existing_path =
        env::split_paths(&env::var_os("PATH").unwrap_or("".into())).collect::<Vec<_>>();
    let path = env::join_paths(
        [bin_dir.as_os_str()]
            .into_iter()
            .chain(existing_path.iter().map(|p| p.as_os_str())),
    )
    .unwrap();
    ensure_directory(&bin_dir, true).unwrap();

    ensure_directory(&project_dir, true).unwrap();
    write_file(
        &project_dir.join("pants.toml"),
        false,
        r#"
            [GLOBAL]
            pants_version = "2.18.0"
            [anonymous-telemetry]
            enabled = false
            "#,
    )
    .unwrap();

    softlink(scie_pants_scie, &bin_dir.join("foo")).unwrap();
    softlink(scie_pants_scie, &project_dir.join("bar")).unwrap();
    let absolute_argv0_path = check_pants_bin_name_chroot.path().join("baz");
    softlink(scie_pants_scie, &absolute_argv0_path).unwrap();

    let assert_pants_bin_name = |argv0: &str, expected_bin_name: &str, extra_envs: Vec<(_, _)>| {
        let output = String::from_utf8(
            execute(
                Command::new(argv0)
                    .arg("help-advanced")
                    .arg("global")
                    .env("PATH", &path)
                    .envs(extra_envs)
                    .current_dir(&project_dir)
                    .stdout(Stdio::piped()),
            )
            .unwrap()
            .stdout,
        )
        .unwrap();
        let expected_output =
            format!("current value: {expected_bin_name} (from env var PANTS_BIN_NAME)");
        assert!(
            output.contains(&expected_output),
            "Expected:{EOL}{expected_output}{EOL}STDOUT was:{EOL}{output}",
        );
    };

    assert_pants_bin_name("foo", "foo", vec![]);
    assert_pants_bin_name("./bar", "./bar", vec![]);

    let absolute_argv0 = absolute_argv0_path.to_str().unwrap();
    assert_pants_bin_name(absolute_argv0, absolute_argv0, vec![]);
    assert_pants_bin_name(absolute_argv0, "spam", vec![("PANTS_BIN_NAME", "spam")]);
}

fn test_pants_bootstrap_handling(scie_pants_scie: &Path) {
    integration_test!("Checking .pants.bootstrap handling ignores bash functions");
    // N.B.: We run this test after 1st having run the test above to ensure pants is already
    // bootstrapped so that we don't get stderr output from that process. We also use
    // `--no-pantsd` to avoid spurious pantsd startup stderr log lines just in case pantsd found
    // a need to restart.
    let output = execute(
        Command::new(scie_pants_scie)
            .args([
                "--no-pantsd",
                // Work around https://github.com/pantsbuild/pants/issues/21863, which results in
                // irrelevant nailgun-related log lines
                "--no-process-execution-local-enable-nailgun",
                "-V",
            ])
            .stderr(Stdio::piped()),
    )
    .unwrap();
    assert!(
        output.stderr.is_empty(),
        "Expected no warnings to be printed when handling .pants.bootstrap, found:\n{warnings}",
        warnings = String::from_utf8_lossy(&output.stderr)
    );
}

fn test_tools_pex_reproducibility(
    workspace_root: &Path,
    tools_pex_path: &Path,
    tools_pex_mismatch_warn: bool,
) {
    integration_test!(
        "Verifying the tools.pex built by the package crate matches the tools.pex built by \
            Pants"
    );
    let pants_tools_pex_path = workspace_root.join("dist").join("tools").join("tools.pex");
    let pants_tools_pex_fingerprint = fingerprint(&pants_tools_pex_path).unwrap();
    let our_tools_pex_fingerprint = fingerprint(tools_pex_path).unwrap();
    if !tools_pex_mismatch_warn {
        assert_eq!(our_tools_pex_fingerprint, pants_tools_pex_fingerprint);
    } else if our_tools_pex_fingerprint != pants_tools_pex_fingerprint {
        log!(
            Color::Yellow,
            "The tools.pex generated by Pants does not match ours:{eol}\
                Ours:  {our_tools_path}{eol}\
                ->     {ours}{eol}\
                Pants: {pants_tools_path}{eol}\
                ->     {pants}{eol}",
            our_tools_path = tools_pex_path.display(),
            ours = our_tools_pex_fingerprint,
            pants_tools_path = pants_tools_pex_path.display(),
            pants = pants_tools_pex_fingerprint,
            eol = EOL,
        );
    }
}

fn test_pants_bootstrap_tools(scie_pants_scie: &Path) {
    integration_test!("Verifying PANTS_BOOTSTRAP_TOOLS works");
    execute(
        Command::new(scie_pants_scie)
            .env("PANTS_BOOTSTRAP_TOOLS", "1")
            .args(["bootstrap-cache-key"]),
    )
    .unwrap();
}

fn test_pants_2_25_using_python_3_11(scie_pants_scie: &Path) {
    integration_test!("Verifying we can run Pants 2.25+, which uses Python 3.11");
    // Pants 2.25 is built on macOS 13 (x86-64) and 14 (arm64), and only truly supports those
    // versions. See https://github.com/pantsbuild/pants/pull/21655
    if is_macos_thats_too_old(13, 14) {
        log!(
            Color::Yellow,
            "Pants 2.25 cannot run on this version of macOS => skipping"
        );
        return;
    }

    let pants_version = "2.25.0.dev0";
    let output = execute(
        Command::new(scie_pants_scie)
            .env("PANTS_VERSION", pants_version)
            .arg("-V")
            .stdout(Stdio::piped()),
    )
    .unwrap();
    let stdout = decode_output(output.stdout).unwrap();
    assert!(
        stdout.contains(pants_version),
        "STDOUT did not contain '{pants_version}':\n{stdout}"
    );
}

fn test_python_repos_repos(scie_pants_scie: &Path) {
    integration_test!(
        "Verifying --python-repos-repos is used prior to Pants 2.13 (no warnings should be \
            issued by Pants)"
    );
    execute(
        Command::new(scie_pants_scie)
            .env("PANTS_VERSION", "2.12.1")
            .args(["--no-verify-config", "-V"]),
    )
    .unwrap();
}

fn test_initialize_new_pants_project(scie_pants_scie: &Path) {
    integration_test!("Verifying initializing a new Pants project works");
    // This test uses the latest Pants version (as it runs in a repo with no pants.toml).
    // So we must only run it on appropriate macos versions.
    if is_macos_thats_too_old(13, 14) {
        log!(
            Color::Yellow,
            "The latest version of Pants cannot run on this version of macOS => skipping"
        );
        return;
    }
    let new_project_dir = create_tempdir().unwrap();
    execute(Command::new("git").arg("init").arg(new_project_dir.path())).unwrap();
    let project_subdir = new_project_dir.path().join("subdir").join("sub-subdir");
    ensure_directory(&project_subdir, false).unwrap();
    execute_with_input(
        Command::new(scie_pants_scie)
            .arg("-V")
            .current_dir(project_subdir),
        "yes".as_bytes(),
    )
    .unwrap();
    assert!(new_project_dir.path().join("pants.toml").is_file());
}

fn test_set_pants_version(scie_pants_scie: &Path) {
    integration_test!("Verifying setting the Pants version on an existing Pants project works");
    let existing_project_dir = create_tempdir().unwrap();
    touch(&existing_project_dir.path().join("pants.toml")).unwrap();
    execute_with_input(
        Command::new(scie_pants_scie)
            .arg("-V")
            .current_dir(existing_project_dir.path()),
        "Y".as_bytes(),
    )
    .unwrap();
}

fn test_ignore_empty_pants_version(scie_pants_scie: &Path) {
    integration_test!("Verifying ignoring PANTS_VERSION when set to empty string");

    let tmpdir = create_tempdir().unwrap();

    let pants_release = "2.18.0";
    let pants_toml_content = format!(
        r#"
        [GLOBAL]
        pants_version = "{pants_release}"
        "#
    );
    let pants_toml = tmpdir.path().join("pants.toml");
    write_file(&pants_toml, false, pants_toml_content).unwrap();

    let output = execute(
        Command::new(scie_pants_scie)
            .arg("-V")
            .env("PANTS_VERSION", "")
            .current_dir(&tmpdir)
            .stdout(Stdio::piped()),
    );
    assert_eq!(
        pants_release,
        decode_output(output.unwrap().stdout).unwrap().trim()
    );
}

fn test_pants_from_pex_version(scie_pants_scie: &Path) {
    integration_test!("Verify scie-pants can use Pants released as a 'local' PEX");

    let tmpdir = create_tempdir().unwrap();

    let pants_release = "2.18.0";
    let pants_toml_content = format!(
        r#"
        [GLOBAL]
        pants_version = "{pants_release}"
        "#
    );
    let pants_toml = tmpdir.path().join("pants.toml");
    write_file(&pants_toml, false, pants_toml_content).unwrap();

    let output = execute(
        Command::new(scie_pants_scie)
            .arg("-V")
            .current_dir(&tmpdir)
            .stdout(Stdio::piped()),
    );
    let expected_message = pants_release;
    let stdout = decode_output(output.unwrap().stdout).unwrap();
    assert!(
        stdout.contains(expected_message),
        "STDOUT did not contain '{expected_message}':\n{stdout}"
    );
}

fn test_pants_from_bad_pex_version(scie_pants_scie: &Path) {
    integration_test!(
        "Verify the output of scie-pants is user-friendly if they provide an invalid pants version"
    );

    let tmpdir = create_tempdir().unwrap();

    let pants_release = "2.19";
    let pants_toml_content = format!(
        r#"
        [GLOBAL]
        pants_version = "{pants_release}"
        "#
    );
    let pants_toml = tmpdir.path().join("pants.toml");
    write_file(&pants_toml, false, pants_toml_content).unwrap();

    let err = execute(
        Command::new(scie_pants_scie)
            .arg("-V")
            .current_dir(&tmpdir)
            .stderr(Stdio::piped()),
    )
    .unwrap_err();

    let error_text = err.to_string();
    assert!(error_text
        .contains("Pants version must be a full version, including patch level, got: `2.19`."));
    assert!(error_text.contains(
        "Please add `.<patch_version>` to the end of the version. For example: `2.18` -> `2.18.0`."
    ));
}

fn test_use_in_repo_with_pants_script(scie_pants_scie: &Path, clone_root: &TempDir) {
    integration_test!("Verify scie-pants can be used as `pants` in a repo with the `pants` script");
    // This verifies a fix for https://github.com/pantsbuild/scie-pants/issues/28.
    execute(
        Command::new("git")
            .args(["clone", "https://github.com/pantsbuild/example-django"])
            .current_dir(clone_root.path()),
    )
    .unwrap();

    let django_dir = clone_root.path().join("example-django");
    execute(
        Command::new("git")
            .args(["checkout", "ff20d1126b5d67b6a77f7d6a39f3063d1897ceb4"])
            .current_dir(&django_dir),
    )
    .unwrap();

    let bin_dir = clone_root.path().join("bin");
    ensure_directory(&bin_dir, false).unwrap();
    copy(scie_pants_scie, bin_dir.join("pants").as_path()).unwrap();
    let new_path = if let Ok(existing_path) = env::var("PATH") {
        format!(
            "{bin_dir}{path_sep}{existing_path}",
            bin_dir = bin_dir.display(),
            path_sep = PATHSEP
        )
    } else {
        format!("{bin_dir}", bin_dir = bin_dir.display())
    };
    execute(
        Command::new("pants")
            .arg("-V")
            .env("PATH", new_path)
            .current_dir(django_dir),
    )
    .unwrap();
}

fn test_dot_env_loading(scie_pants_scie: &Path, clone_root: &TempDir) {
    integration_test!(
        "Verify `.env` loading works (example-django should down grade to Pants 2.12.1)"
    );
    write_file(
        &clone_root.path().join(".env"),
        false,
        "PANTS_VERSION=2.12.1",
    )
    .unwrap();
    execute(
        Command::new(scie_pants_scie)
            .arg("-V")
            .current_dir(clone_root.path().join("example-django")),
    )
    .unwrap();
}

fn test_dot_env_error(scie_pants_scie: &Path) {
    integration_test!("Verify `.env` loading emits errors if invalid");

    let tempdir = create_tempdir().unwrap();
    write_file(
        &tempdir.path().join(".env"),
        false,
        "CABBAGE=cabbagee\ntotally invalid line\nPOTATO=potato",
    )
    .unwrap();

    assert_stderr_output(
        Command::new(scie_pants_scie)
            .arg("-V")
            .current_dir(tempdir.path()),
        vec!["requested .env files be loaded but there was an error doing so: Parsing Error: Error { input: \"invalid line"],
        ExpectedResult::Failure
    );
}

fn test_pants_source_mode(
    scie_pants_scie: &Path,
    clone_dir: &Path,
    pants_2_25_0_dev1_clone_dir: &Path,
    venv_dir: &Path,
    pants_2_25_0_dev1_venv_dir: &Path,
) {
    integration_test!("Verify PANTS_SOURCE mode.");
    // NB. we assume that these directories are setup perfectly if they exist. A possible failure
    // mode is the symlinks to python interpreters in the venv; if the system changes to make them
    // invalid, we start getting errors like `${pants_2_25_0_dev1_venv_dir}/.../bin/python: No such file
    // or directory`. This can occur in practice with cross-runner caching and the runner updating,
    // but our cache key is designed to avoid this (see `build_it_cache_key` step in ci.yml).
    if !pants_2_25_0_dev1_clone_dir.exists() || !pants_2_25_0_dev1_venv_dir.exists() {
        let clone_root_tmp = create_tempdir().unwrap();
        let clone_root_path = clone_root_tmp
            .path()
            .to_str()
            .with_context(|| {
                format!("Failed to convert clone root path to UTF-8 string: {clone_root_tmp:?}")
            })
            .unwrap();
        execute(Command::new("git").args(["init", clone_root_path])).unwrap();
        // N.B.: The release_2.25.0.dev1 tag has sha b4c218ba0820e4673f8d9ad72b80e0285f4d5604 and we
        // must pass a full sha to use the shallow fetch trick.
        const PANTS_2_25_0_DEV1_SHA: &str = "b4c218ba0820e4673f8d9ad72b80e0285f4d5604";
        execute(
            Command::new("git")
                .args([
                    "fetch",
                    "--depth",
                    "1",
                    "https://github.com/pantsbuild/pants",
                    PANTS_2_25_0_DEV1_SHA,
                ])
                .current_dir(clone_root_tmp.path()),
        )
        .unwrap();
        execute(
            Command::new("git")
                .args(["reset", "--hard", PANTS_2_25_0_DEV1_SHA])
                .current_dir(clone_root_tmp.path()),
        )
        .unwrap();
        write_file(
            clone_root_tmp.path().join("patch").as_path(),
            false,
            r#"
diff --git a/build-support/pants_venv b/build-support/pants_venv
index 90fa82f6d3..e4f7e97a95 100755
--- a/build-support/pants_venv
+++ b/build-support/pants_venv
@@ -13,6 +13,8 @@ REQUIREMENTS=(

 platform=$(uname -mps)

+echo >&2 "The ${SCIE_PANTS_TEST_MODE:-Pants 2.25.0.dev1 clone} is working."
+
 function venv_dir() {
   # Include the entire version string in order to differentiate e.g. PyPy from CPython.
   # Fingerprinting uname and python output avoids shebang length limits and any odd chars.
@@ -23,7 +25,7 @@ function venv_dir() {

   # NB: We house these outside the working copy to avoid needing to gitignore them, but also to
   # dodge https://github.com/hashicorp/vagrant/issues/12057.
-  echo "${HOME}/.cache/pants/pants_dev_deps/${venv_fingerprint}.venv"
+  echo "${PANTS_VENV_DIR_PREFIX:-${HOME}/.cache/pants/pants_dev_deps}/${venv_fingerprint}.venv"
 }

 function activate_venv() {
diff --git a/pants b/pants
index ba49cc133f..870a35f028 100755
--- a/pants
+++ b/pants
@@ -76,4 +76,5 @@ function exec_pants_bare() {
     exec ${PANTS_PREPEND_ARGS:-} "$(venv_dir)/bin/python" ${DEBUG_ARGS} "${PANTS_PY_EXE}" "$@"
 }

+echo >&2 "Pants from sources argv: $@."
 exec_pants_bare "$@"
diff --git a/src/python/pants/VERSION b/src/python/pants/VERSION
index 796b3cddd2..aef0e649bb 100644
--- a/src/python/pants/VERSION
+++ b/src/python/pants/VERSION
@@ -1 +1 @@
-2.25.0.dev1
+2.25.0.dev1+Custom-Local
"#,
        )
        .unwrap();
        execute(
            Command::new("git")
                .args(["apply", "patch"])
                .current_dir(clone_root_tmp.path()),
        )
        .unwrap();
        let venv_root_tmp = create_tempdir().unwrap();
        execute(
            Command::new("./pants")
                .arg("-V")
                .env("PANTS_VENV_DIR_PREFIX", venv_root_tmp.path())
                .current_dir(clone_root_tmp.path()),
        )
        .unwrap();

        remove_dir(
            clone_root_tmp
                .path()
                .join("src")
                .join("rust")
                .join("engine")
                .join("target")
                .as_path(),
        )
        .unwrap();
        ensure_directory(clone_dir, true).unwrap();
        rename(&clone_root_tmp.keep(), pants_2_25_0_dev1_clone_dir).unwrap();
        ensure_directory(venv_dir, true).unwrap();
        rename(&venv_root_tmp.keep(), pants_2_25_0_dev1_venv_dir).unwrap();
    }

    assert_stderr_output(
        Command::new(scie_pants_scie)
            .arg("-V")
            .env("PANTS_SOURCE", pants_2_25_0_dev1_clone_dir)
            .env("SCIE_PANTS_TEST_MODE", "PANTS_SOURCE mode")
            .env("PANTS_VENV_DIR_PREFIX", pants_2_25_0_dev1_venv_dir),
        vec![
            "The PANTS_SOURCE mode is working.",
            "Pants from sources argv: --no-verify-config -V.",
        ],
        ExpectedResult::Success,
    );

    let invalid_pants_clone_dir = pants_2_25_0_dev1_clone_dir.join("xyzzy");
    assert_stderr_output(
        Command::new(scie_pants_scie)
            .arg("-V")
            .env("PANTS_SOURCE", &invalid_pants_clone_dir)
            .env("SCIE_PANTS_TEST_MODE", "PANTS_SOURCE mode")
            .env("PANTS_VENV_DIR_PREFIX", pants_2_25_0_dev1_venv_dir),
        vec![
            &format!("Error: Unable to find the `pants` runner script in the requested Pants source directory `{}`. \
            Running Pants from sources was enabled because the `PANTS_SOURCE` environment variable is set.",
            invalid_pants_clone_dir.display())
        ],
        ExpectedResult::Failure,
    );
}

fn test_pants_from_sources_mode(
    scie_pants_scie: &Path,
    pants_2_25_0_dev1_clone_dir: &Path,
    pants_2_25_0_dev1_venv_dir: &Path,
) {
    integration_test!("Verify pants_from_sources mode.");
    let side_by_side_root = create_tempdir().unwrap();
    let pants_dir = side_by_side_root.path().join("pants");
    softlink(pants_2_25_0_dev1_clone_dir, &pants_dir).unwrap();
    let user_repo_dir = side_by_side_root.path().join("user-repo");
    ensure_directory(&user_repo_dir, true).unwrap();
    touch(user_repo_dir.join("pants.toml").as_path()).unwrap();
    touch(user_repo_dir.join("BUILD_ROOT").as_path()).unwrap();

    let pants_from_sources = side_by_side_root.path().join("pants_from_sources");
    softlink(scie_pants_scie, &pants_from_sources).unwrap();

    assert_stderr_output(
        Command::new(&pants_from_sources)
            .arg("-V")
            .env("SCIE_PANTS_TEST_MODE", "pants_from_sources mode")
            .env("PANTS_VENV_DIR_PREFIX", pants_2_25_0_dev1_venv_dir)
            .current_dir(&user_repo_dir),
        vec![
            "The pants_from_sources mode is working.",
            "Pants from sources argv: --no-verify-config -V.",
        ],
        ExpectedResult::Success,
    );

    let invalid_pants_dir = side_by_side_root.path().join("pants-xyzzy");
    rename(&pants_dir, &invalid_pants_dir).unwrap();
    assert_stderr_output(
        Command::new(&pants_from_sources)
            .arg("-V")
            .env("SCIE_PANTS_TEST_MODE", "pants_from_sources mode")
            .env("PANTS_VENV_DIR_PREFIX", pants_2_25_0_dev1_venv_dir)
            .current_dir(&user_repo_dir),
        vec![
            "Error: Unable to find the `pants` runner script in the requested Pants source directory `../pants`. \
            Running Pants from sources was enabled because the Pants launcher was invoked as `pants_from_sources`."
        ],
        ExpectedResult::Failure,
    );
}

fn test_delegate_pants_in_pants_repo(
    scie_pants_scie: &Path,
    pants_2_25_0_dev1_clone_dir: &PathBuf,
) {
    integration_test!("Verify delegating to `./pants`.");
    assert_stderr_output(
        Command::new(scie_pants_scie)
            .arg("-V")
            .env("SCIE_PANTS_TEST_MODE", "delegate_bootstrap mode")
            .current_dir(pants_2_25_0_dev1_clone_dir),
        vec![
            "The delegate_bootstrap mode is working.",
            "Pants from sources argv: -V.",
        ],
        ExpectedResult::Success,
    );
}

fn test_use_pants_release_in_pants_repo(
    scie_pants_scie: &Path,
    pants_2_25_0_dev1_clone_dir: &PathBuf,
) {
    let pants_release = "2.25.0.dev1";
    integration_test!("Verify usage of Pants {pants_release} on the pants repo.");
    let (output, stderr) = assert_stderr_output(
        Command::new(scie_pants_scie)
            .arg("help")
            .env("PANTS_VERSION", pants_release)
            .env(
                "PANTS_BACKEND_PACKAGES",
                "-[\
                    'internal_plugins.test_lockfile_fixtures',\
                    'pants_explorer.server',\
                    ]",
            )
            .current_dir(pants_2_25_0_dev1_clone_dir)
            .stdout(Stdio::piped()),
        vec![],
        ExpectedResult::Success,
    );
    let expected_message = pants_release;
    let stdout = decode_output(output.stdout).unwrap();
    assert!(
        stdout.contains(expected_message),
        "STDOUT did not contain '{expected_message}':\n{stdout}"
    );
    let unexpected_message = "Pants from sources argv";
    assert!(
        !stderr.contains(unexpected_message),
        "STDERR unexpectedly contained '{unexpected_message}':\n{stderr}"
    );
}

fn test_python38_used_for_old_pants(scie_pants_scie: &Path) {
    integration_test!("Verifying Python 3.8 is selected for Pants older than 2.5.0");
    let mut command = Command::new(scie_pants_scie);
    command
        .env("PANTS_VERSION", "1.30.5rc1")
        .env(
            "PANTS_BACKEND_PACKAGES",
            "-[\
                'pants.backend.python.typecheck.mypy',\
                'pants.backend.shell',\
                'pants.backend.shell.lint.shellcheck',\
                'pants.backend.shell.lint.shfmt',\
                ]",
        )
        .args(["--no-verify-config", "--version"]);
    if Platform::MacOSX86_64 == *CURRENT_PLATFORM {
        // For unknown reasons, macOS x86_64 hangs in CI if this last test, like all prior tests
        // nonetheless!, is run with pantsd enabled mode.
        command.arg("--no-pantsd");
    }
    execute(&mut command).unwrap();
}

fn test_self_update(scie_pants_scie: &Path) {
    integration_test!("Verifying self update works");
    // N.B.: There should never be a newer release in CI; so this should always gracefully noop
    // noting no newer release was available.
    execute(Command::new(scie_pants_scie).env("SCIE_BOOT", "update")).unwrap();
}

fn test_self_downgrade(scie_pants_scie: &Path) {
    integration_test!("Verifying downgrade works");
    // Additionally, we exercise using a relative path to the scie-jump binary which triggered
    // https://github.com/pantsbuild/scie-pants/issues/38 in the past.
    let tmpdir = create_tempdir().unwrap();
    let scie_pants_basename = scie_pants_scie.file_name().unwrap();
    let scie_pants = tmpdir.path().join(scie_pants_basename);
    copy(scie_pants_scie, &scie_pants).unwrap();
    execute(
        Command::new(PathBuf::from(".").join(scie_pants_basename))
            .env("SCIE_BOOT", "update")
            .arg("0.1.8")
            .current_dir(tmpdir.path()),
    )
    .unwrap();
}

fn test_caching_issue_129(scie_pants_scie: &Path) {
    integration_test!(
        "Verifying the build root does not influence caching ({issue})",
        issue = issue_link!(129)
    );
    let tmpdir = create_tempdir().unwrap();

    let scie_base = tmpdir.path().join("nce");

    let pants_toml = r#"
    [GLOBAL]
    pants_version = "2.18.0"
    [anonymous-telemetry]
    enabled = false
    "#;

    let one = tmpdir.path().join("one");
    ensure_directory(&one, false).unwrap();
    write_file(&one.join("pants.toml"), false, pants_toml).unwrap();
    execute(
        Command::new(scie_pants_scie)
            .arg("-V")
            .env("SCIE_BASE", &scie_base)
            .current_dir(&one),
    )
    .unwrap();

    let two = tmpdir.path().join("two");
    ensure_directory(&two, false).unwrap();
    write_file(&two.join("pants.toml"), false, pants_toml).unwrap();
    execute(
        Command::new(scie_pants_scie)
            .arg("-V")
            .env("SCIE_BASE", &scie_base)
            .current_dir(&two),
    )
    .unwrap();

    #[derive(Debug, Eq, PartialEq)]
    enum LockType {
        Configure,
        Install,
    }
    let binding_locks = walkdir::WalkDir::new(scie_base)
        .sort_by_file_name()
        .into_iter()
        .filter_map(|entry| match entry {
            Ok(dir_entry) => {
                if !dir_entry.file_type().is_file() {
                    return None;
                }
                if let Some(file_name) = dir_entry.file_name().to_str() {
                    if let Some(parent_dir) = dir_entry.path().parent() {
                        if let Some(parent_dir_name) = parent_dir.file_name() {
                            if "locks" != parent_dir_name {
                                return None;
                            }
                        }
                        if !file_name.ends_with(".lck") {
                            return None;
                        }
                        if file_name.starts_with("configure-") {
                            return Some(LockType::Configure);
                        }
                        if file_name.starts_with("install-") {
                            return Some(LockType::Install);
                        }
                    }
                }
                None
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(vec![LockType::Configure, LockType::Install], binding_locks)
}

fn test_custom_pants_toml_issue_153(scie_pants_scie: &Path) {
    integration_test!(
        "Verifying the PANTS_TOML env var is respected ({issue})",
        issue = issue_link!(153)
    );

    let tmpdir = create_tempdir().unwrap();

    let buildroot = tmpdir.path().join("buildroot");
    touch(&buildroot.join("BUILD_ROOT")).unwrap();

    let pants_toml_content = r#"
    [GLOBAL]
    pants_version = "2.17.0.dev4"
    backend_packages = ["pants.backend.python"]
    [anonymous-telemetry]
    enabled = false
    "#;
    let pants_toml = tmpdir.path().join("elsewhere").join("pants.toml");
    write_file(&pants_toml, false, pants_toml_content).unwrap();

    let buildroot_subdir = buildroot.join("subdir");
    ensure_directory(&buildroot_subdir, false).unwrap();

    let output = execute(
        Command::new(scie_pants_scie)
            .arg("-V")
            .env("PANTS_TOML", &pants_toml)
            .env("PANTS_CONFIG_FILES", &pants_toml)
            .current_dir(&buildroot_subdir)
            .stdout(Stdio::piped()),
    )
    .unwrap();
    assert_eq!(
        "2.17.0.dev4",
        String::from_utf8(output.stdout.to_vec()).unwrap().trim()
    );

    let build_content = r#"
python_requirement(name="cowsay", requirements=["cowsay==5.0"])
pex_binary(name="moo", script="cowsay", dependencies=[":cowsay"])
    "#;
    write_file(&buildroot_subdir.join("BUILD"), false, build_content).unwrap();
    let output = execute(
        Command::new(scie_pants_scie)
            .args(["list", ":"])
            .env("PANTS_TOML", &pants_toml)
            .env("PANTS_CONFIG_FILES", &pants_toml)
            .current_dir(&buildroot_subdir)
            .stdout(Stdio::piped()),
    )
    .unwrap();

    let expected_output = r#"
subdir:cowsay
subdir:moo
    "#;
    assert_eq!(
        expected_output.trim(),
        String::from_utf8(output.stdout.to_vec()).unwrap().trim()
    );

    let dot_env_content = format!(
        r#"
export PANTS_TOML={pants_toml}
export PANTS_CONFIG_FILES=${{PANTS_TOML}}
        "#,
        pants_toml = pants_toml.display()
    );
    write_file(&buildroot.join(".env"), false, dot_env_content).unwrap();
    let output = execute(
        Command::new(scie_pants_scie)
            .args(["list", ":"])
            .current_dir(&buildroot_subdir)
            .stdout(Stdio::piped()),
    )
    .unwrap();
    assert_eq!(
        expected_output.trim(),
        String::from_utf8(output.stdout.to_vec()).unwrap().trim()
    );
}

fn test_pants_native_client_perms_issue_182(scie_pants_scie: &Path) {
    integration_test!(
        "Verifying scie-pants sets executable perms on the Pants native client binary when \
        present ({issue})",
        issue = issue_link!(182)
    );

    let tmpdir = create_tempdir().unwrap();

    let pants_release = "2.17.0a1";
    let pants_toml_content = format!(
        r#"
        [GLOBAL]
        pants_version = "{pants_release}"
        "#
    );
    let pants_toml = tmpdir.path().join("pants.toml");
    write_file(&pants_toml, false, pants_toml_content).unwrap();

    let output = execute(
        Command::new(scie_pants_scie)
            .arg("-V")
            .current_dir(&tmpdir)
            .stdout(Stdio::piped()),
    );
    assert_eq!(
        pants_release,
        decode_output(output.unwrap().stdout).unwrap().trim()
    );
}

#[cfg(unix)]
fn test_non_utf8_env_vars_issue_198(scie_pants_scie: &Path) {
    integration_test!(
        "Verifying scie-pants is robust to environments with non-utf8 env vars present ({issue})",
        issue = issue_link!(198)
    );

    let tmpdir = create_tempdir().unwrap();

    let pants_release = "2.17.0a1";
    let pants_toml_content = format!(
        r#"
        [GLOBAL]
        pants_version = "{pants_release}"
        "#
    );
    let pants_toml = tmpdir.path().join("pants.toml");
    write_file(&pants_toml, false, pants_toml_content).unwrap();

    use std::os::unix::ffi::OsStringExt;
    env::set_var("FOO", OsString::from_vec(vec![b'B', 0xa5, b'R']));

    let err = execute(
        Command::new(scie_pants_scie)
            .arg("-V")
            .env("RUST_LOG", "trace")
            .stderr(Stdio::piped())
            .current_dir(&tmpdir),
    )
    .unwrap_err();
    let error_text = err.to_string();
    // N.B.: This is a very hacky way to confirm the `scie-jump` is done processing env vars and has
    // exec'd the `scie-pants` native client; which then proceeds to choke on env vars in the same
    // way scie-jump <= 0.11.0 did using `env::vars()`.
    assert!(Regex::new(concat!(
        r#"exe: ".*/bindings/venvs/2\.17\.0a1/lib/python3\.9/"#,
        r#"site-packages/pants/bin/native_client""#
    ))
    .unwrap()
    .find(&error_text)
    .is_some());
    assert!(error_text.contains("[DEBUG TimerFinished] jump::prepare_boot(), Elapsed="));
    assert!(error_text
        .contains(r#"panicked at 'called `Result::unwrap()` on an `Err` value: "B\xA5R"'"#));

    // The error path we test below requires flowing through the pantsd path via PyNailgunClient.
    let err = execute(
        Command::new(scie_pants_scie)
            .arg("--pantsd")
            .arg("-V")
            .env("PANTS_NO_NATIVE_CLIENT", "1")
            .stderr(Stdio::piped())
            .current_dir(&tmpdir),
    )
    .unwrap_err();
    // Here we're asking the native client to exit very early before it processed `env::vars()`; so
    // the execution makes it into Python code that calls
    // `PyNailgunClient(...).execute(command, args, modified_env)`. That's Rust code implementing a
    // Python extension object that also wrongly assumes utf8 when converting env vars.
    assert!(err.to_string().contains(concat!(
        r#"UnicodeEncodeError: 'utf-8' codec can't encode character '\udca5' in "#,
        "position 1: surrogates not allowed"
    )));

    let output = execute(
        Command::new(scie_pants_scie)
            .arg("--no-pantsd")
            .arg("-V")
            .env("PANTS_NO_NATIVE_CLIENT", "1")
            .stdout(Stdio::piped())
            .current_dir(&tmpdir),
    )
    .unwrap();
    assert_eq!(pants_release, decode_output(output.stdout).unwrap().trim());

    env::remove_var("FOO");
}

fn test_bad_boot_error_text(scie_pants_scie: &Path) {
    integration_test!(
        "Verifying the output of scie-pants is user-friendly if they provide an unexpected SCIE_BOOT argument",
    );
    let (_, stderr) = assert_stderr_output(
        Command::new(scie_pants_scie).env("SCIE_BOOT", "does-not-exist"),
        vec![
            "`SCIE_BOOT=does-not-exist` was found in the environment",
            // the various boot commands we want users to know about
            "\n<default> ",
            "\nbootstrap-tools ",
            "\nupdate ",
        ],
        ExpectedResult::Failure,
    );

    // Check that boot commands that users shouldn't see (used internally, only) aren't included.
    for bad_boot in ["pants", "pants-debug"] {
        let pattern = format!("\n{bad_boot} ");
        assert!(
            !stderr.contains(&pattern),
            "STDERR contains '{pattern:?} ' at the start of a line, potentially referring to SCIE_BOOT=pants command that shouldn't appear:\n{stderr}"
        );
    }
}

fn test_pants_bootstrap_urls(scie_pants_scie: &Path) {
    integration_test!(
      "Verifying PANTS_BOOTSTRAP_URLS is used for both CPython interpreter and Pants PEX ({issue})",
      issue = issue_link!(243)
    );

    // This test runs in 4 parts:
    //
    // 0. Setup tempdirs, common values etc.
    // 1. Verify interpreter download uses URL (by checking errors with a non-existent URL)
    // 2. The same, but for the Pants PEX
    // 3. Verify that specifying valid URLs works too (no good if we're just succesfully failing)

    // Part 0: Setup
    let tmpdir = create_tempdir().unwrap();

    // A fresh directory to ensure the downloads happen fresh.
    let scie_base = tmpdir.path().join("scie-base");

    // Set up a pants.toml
    let pants_release = "2.18.0rc1";
    let pants_toml_content = format!(
        r#"
        [GLOBAL]
        pants_version = "{pants_release}"
        "#
    );
    let project_dir = tmpdir.path().join("project");
    let pants_toml = project_dir.join("pants.toml");
    write_file(&pants_toml, false, pants_toml_content).unwrap();

    // The file that we'll plop our URL overrides into...
    let urls_json = tmpdir.path().join("urls.json");
    // ... plus helpers to write to it, we start with the `ptex` key/value of this scie-pants's
    // `SCIE=inspect` output (which will be the Python interpreters and their default URLs), but
    // allow the tests to update it.
    let output = execute(
        Command::new(scie_pants_scie)
            .env("SCIE", "inspect")
            .stdout(Stdio::piped()),
    )
    .unwrap();
    let mut ptex_json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    ptex_json
        .as_object_mut()
        .unwrap()
        .retain(|key, _| key == "ptex");

    let write_urls_json =
        |update_ptex_map: &dyn Fn(&mut serde_json::Map<String, serde_json::Value>)| {
            let mut json = ptex_json.clone();
            // Transform the "ptex": {...} map as appropriate.
            update_ptex_map(json["ptex"].as_object_mut().unwrap());
            write_file(&urls_json, false, serde_json::to_vec(&json).unwrap()).unwrap();
        };

    // Reference data for the Pants we'll try to install (NB. we have to force new-enough version of
    // Pants to install via PEXes, older versions go via PyPI which isn't managed by
    // PANTS_BOOTSTRAP_URLS)
    let platforms = [
        "darwin_arm64",
        "darwin_x86_64",
        "linux_aarch64",
        "linux_x86_64",
    ];
    let pexes = platforms
        .iter()
        .map(|platform| format!("pants.{pants_release}-cp39-{platform}.pex"))
        .collect::<Vec<_>>();

    // we run the exact same command each time
    let mut command = Command::new(scie_pants_scie);
    command
        .arg("-V")
        .env("PANTS_BOOTSTRAP_URLS", &urls_json)
        .env("SCIE_BASE", &scie_base)
        .current_dir(&project_dir);

    // Part 1: Validate that we attempt to download the CPython interpreter from (invalid) override
    // URLs

    // Set every ptex file value to the substitute URL, that doesn't exist
    let doesnt_exist_interpreter = tmpdir.path().join("doesnt-exist-interpreter");
    let doesnt_exist_interpreter_url = format!("file://{}", doesnt_exist_interpreter.display());
    write_urls_json(&|ptex_map| {
        for value in ptex_map.values_mut() {
            *value = doesnt_exist_interpreter_url.clone().into();
        }
    });

    assert_stderr_output(
        &mut command,
        vec![&format!("Failed to fetch {doesnt_exist_interpreter_url}")],
        ExpectedResult::Failure,
    );

    // Part 2: Validate that we attempt to download Pants PEXes from (invalid) override URLs

    // Leave the interpreters and add new URLs for the various PEXes that this test might need (all
    // the different platforms); as above, the URL doesn't exist
    let doesnt_exist_pex = tmpdir.path().join("doesnt-exist-pex");
    let doesnt_exist_pex_url = format!("file://{}", doesnt_exist_pex.display());
    write_urls_json(&|ptex_map| {
        for pex in &pexes {
            ptex_map.insert(pex.clone(), doesnt_exist_pex_url.clone().into());
        }
    });

    assert_stderr_output(
        &mut command,
        vec![
            &format!("Failed to determine release URL for Pants: {pants_release}: pants.{pants_release}-cp3"),
            &format!(".pex: URL check failed, from PANTS_BOOTSTRAP_URLS: {doesnt_exist_pex_url}: <urlopen error [Errno 2] No such file or directory: "),
        ],
        ExpectedResult::Failure,
    );

    // Part 3: Validate that we can bootstrap pants fully from these override URLs (by manually
    // re-specifying the defaults)
    write_urls_json(&|ptex_map| {
        for pex in &pexes {
            ptex_map.insert(pex.clone(), format!("https://github.com/pantsbuild/pants/releases/download/release_{pants_release}/{pex}").into());
        }
    });

    let output = execute(command.stdout(Stdio::piped())).unwrap();
    let stdout = decode_output(output.stdout).unwrap();
    assert!(stdout.contains(pants_release));
}

fn test_pants_bootstrap_stdout_silent(scie_pants_scie: &Path) {
    integration_test!(
        "Verifying scie-pants bootstraps Pants without any output on stdout ({issue})",
        issue = issue_link!(20315, "pantsbuild/pants")
    );
    let tmpdir = create_tempdir().unwrap();

    let scie_base_dir = tmpdir.path().join("scie-base");

    let pants_release = "2.19.1";
    let pants_toml_content = format!(
        r#"
        [GLOBAL]
        pants_version = "{pants_release}"
        "#
    );
    let project_dir = tmpdir.path().join("project");
    let pants_toml = project_dir.join("pants.toml");
    write_file(&pants_toml, false, pants_toml_content).unwrap();

    // Bootstrap a new unseen version of Pants to verify there is no extra output on stdout besides
    // the requested output from the pants command.
    let (output, _stderr) = assert_stderr_output(
        Command::new(scie_pants_scie)
            .arg("-V")
            .current_dir(&project_dir)
            // Customise where SCIE stores its caches to force a bootstrap...
            .env("SCIE_BASE", scie_base_dir)
            .stdout(Stdio::piped()),
        // ...but still assert bootstrap messages to ensure we actually bootstrapped pants during this execution.
        vec![
            "Bootstrapping Pants 2.19.1",
            "Installing pantsbuild.pants==2.19.1 into a virtual environment at ",
            "New virtual environment successfully created at ",
        ],
        ExpectedResult::Success,
    );
    let stdout = decode_output(output.stdout).unwrap();
    assert!(
        stdout.eq("2.19.1\n"),
        "STDOUT was not '2.19.1':\n{stdout}\n"
    );
}
