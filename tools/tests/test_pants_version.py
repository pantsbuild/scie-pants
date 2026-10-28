# Copyright 2026 Pants project contributors.
# Licensed under the Apache License, Version 2.0 (see LICENSE).

import subprocess
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest
from packaging.version import Version
from scie_pants.install_pants import install_pants_from_pex
from scie_pants.pants_version import TIMEOUT, get_download_url, get_pex_url_and_python_id
from scie_pants.ptex import Ptex

VERSION = Version("2.33.1+example")
PLATFORM = "linux_x86_64"
PEX_NAME = f"pants.{VERSION}-cp314-{PLATFORM}.pex"
GITHUB_URLS = (
    "https://api.github.com/repos/example/builds/actions/artifacts/123/zip",
    "https://github.com/example/builds/actions/runs/456/artifacts/123",
    "https://api.github.com/repos/example/builds/releases/assets/123",
)


@pytest.mark.parametrize(
    "url",
    (
        *GITHUB_URLS,
        GITHUB_URLS[0] + "?secret=synthetic-secret",
        GITHUB_URLS[0] + "#synthetic-secret",
        GITHUB_URLS[0].replace("api.github.com", "synthetic-secret@api.github.com"),
        GITHUB_URLS[0].replace("api.github.com", "api.github.com:443"),
    ),
)
def test_private_github_mapping_defers_validation_to_ptex(url):
    with patch("scie_pants.pants_version.urllib.request.urlopen") as urlopen:
        assert get_download_url(VERSION, PLATFORM, "cp314", {PEX_NAME: url}) == (url, None)
    urlopen.assert_not_called()


def test_explicit_gcs_mapping_defers_validation_to_ptex():
    url = "gs://private-artifacts/releases/pants+example.pex"
    with patch("scie_pants.pants_version.urllib.request.urlopen") as urlopen:
        assert get_download_url(VERSION, PLATFORM, "cp314", {PEX_NAME: url}) == (url, None)
    urlopen.assert_not_called()


@pytest.mark.parametrize(
    "url",
    [
        "https://example.com/pants.pex",
        "http://example.com/pants.pex",
        "https://github.com/example/builds/releases/download/v1/pants.pex",
        "https://api.github.com/repos/example/builds/releases/latest",
        "https://api.github.com/repos/example/builds/actions/artifacts/123",
        "https://api.github.com/repos/example/builds/releases/assets/123/labels",
        "https://api.github.com/repos/example/builds/releases?next=/actions/artifacts/123/zip",
        "https://github.com/example/builds/issues/1?next=/actions/runs/456/artifacts/123",
        "https://api.github.com.attacker.invalid/repos/example/builds/releases/assets/123",
    ],
)
def test_existing_http_mapping_still_uses_head(url):
    with patch("scie_pants.pants_version.urllib.request.urlopen") as urlopen:
        urlopen.return_value.__enter__.return_value.status = 200
        assert get_download_url(VERSION, PLATFORM, "cp314", {PEX_NAME: url}) == (url, None)
    request = urlopen.call_args.args[0]
    assert request.full_url == url
    assert request.method == "HEAD"
    assert urlopen.call_args.kwargs == {"timeout": TIMEOUT}


def test_file_mapping_still_works(tmp_path):
    artifact = tmp_path / "pants.pex"
    artifact.write_bytes(b"example-pex")
    url = artifact.as_uri()
    assert get_download_url(VERSION, PLATFORM, "cp314", {PEX_NAME: url}) == (url, None)


def test_missing_mapping_does_not_fall_back_to_public_release():
    with patch("scie_pants.pants_version.urllib.request.urlopen") as urlopen:
        url, error = get_download_url(VERSION, PLATFORM, "cp314", {"another.pex": "gs://bucket/a"})
    assert url is None
    assert error is not None
    assert "has no URL" in error
    urlopen.assert_not_called()


def test_empty_override_does_not_fall_back_to_public_release():
    with patch("scie_pants.pants_version.urllib.request.urlopen") as urlopen:
        urlopen.return_value.__enter__.return_value.status = 200
        url, error = get_download_url(VERSION, PLATFORM, "cp314", {})
    assert url is None
    assert error is not None
    assert "has no URL" in error
    urlopen.assert_not_called()


def test_default_release_still_uses_head():
    with patch("scie_pants.pants_version.urllib.request.urlopen") as urlopen:
        urlopen.return_value.__enter__.return_value.status = 200
        url, error = get_download_url(VERSION, PLATFORM, "cp314", None)
    assert error is None
    assert (
        url == f"https://github.com/pantsbuild/pants/releases/download/release_{VERSION}/{PEX_NAME}"
    )
    assert urlopen.call_args.args[0].method == "HEAD"


@pytest.mark.parametrize("url", ("gs://private-artifacts/pants.pex", *GITHUB_URLS))
def test_selected_private_mapping_reaches_downloader_unchanged(tmp_path, url):
    mapping = tmp_path / "ptex.json"
    mapping.write_text('{"ptex": {"' + PEX_NAME + '": "' + url + '"}}')
    with patch("scie_pants.pants_version.urllib.request.urlopen") as urlopen:
        assert get_pex_url_and_python_id(Ptex("ptex"), VERSION, PLATFORM, str(mapping)) == (
            url,
            "cp314",
        )
    urlopen.assert_not_called()
    with patch("scie_pants.ptex.subprocess.run") as run:
        Ptex("ptex").fetch_to_fp(url, MagicMock(fileno=lambda: 42))
    assert run.call_args.kwargs == {"args": ["ptex", url], "stdout": 42, "check": True}


@pytest.mark.parametrize("url", ("gs://private-artifacts/pants.pex", *GITHUB_URLS))
@pytest.mark.parametrize("partial_bytes", [b"", b"incomplete-pex"])
def test_failed_private_download_is_never_executed(tmp_path, partial_bytes, url):
    downloaded_paths = []

    def fail_download(source, output):
        assert source == url
        downloaded_paths.append(Path(output.name))
        output.write(partial_bytes)
        raise subprocess.CalledProcessError(1, ["ptex", source])

    ptex = MagicMock(spec=Ptex)
    ptex.fetch_to_fp.side_effect = fail_download
    with (
        patch("scie_pants.install_pants.subprocess.run") as run,
        pytest.raises(SystemExit) as error,
    ):
        install_pants_from_pex(tmp_path, "Pants", VERSION, url, ptex, ())
    assert error.value.code != 0
    ptex.fetch_to_fp.assert_called_once()
    run.assert_not_called()
    assert downloaded_paths
    assert all(not path.exists() for path in downloaded_paths)
    assert not (tmp_path / "pants-install.log").exists()
