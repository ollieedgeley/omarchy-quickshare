"""Omarchy CIDATA configuration adapted from the pinned upstream ISO harness.

See LICENSE for the upstream MIT notice. No application source enters this seed.
"""

# ruff: file-ignore[implicit-namespace-package] - Sibling helpers are imported by the standalone script.

import json
import os
import subprocess  # ruff: ignore[suspicious-subprocess-import] - Fixed argv tools; no shell execution.
from pathlib import Path
from typing import Any, TypedDict, Unpack


class _Partition(TypedDict):
    """Describe an installer partition before adding fixed metadata."""

    start: int
    size: int
    filesystem: str
    mountpoint: str | None
    identifier: str
    flags: list[str]
    subvolumes: list[dict[str, str]]


def _quantity(value: int) -> dict[str, object]:
    return {
        "sector_size": {"unit": "B", "value": 512},
        "unit": "B",
        "value": value,
    }


def _partition(**spec: Unpack[_Partition]) -> dict[str, object]:
    return {
        "btrfs": spec["subvolumes"],
        "dev_path": None,
        "flags": spec["flags"],
        "fs_type": spec["filesystem"],
        "mount_options": ["compress=zstd"] if spec["subvolumes"] else [],
        "mountpoint": spec["mountpoint"],
        "obj_id": spec["identifier"],
        "size": _quantity(spec["size"]),
        "start": _quantity(spec["start"]),
        "status": "create",
        "type": "primary",
    }


def _partitions(manifest: dict[str, Any]) -> list[dict[str, object]]:
    mib = 1024**2
    boot_size = 2 * 1024**3
    return [
        _partition(
            start=mib,
            size=boot_size,
            filesystem="fat32",
            mountpoint="/boot",
            identifier="ea21d3f2-82bb-49cc-ab5d-6f81ae94e18d",
            flags=["boot", "esp"],
            subvolumes=[],
        ),
        _partition(
            start=boot_size + mib,
            size=(manifest["diskGiB"] * 1024**3 - boot_size - 2 * mib),
            filesystem="btrfs",
            mountpoint=None,
            identifier="8c2c2b92-1070-455d-b76a-56263bab24aa",
            flags=[],
            subvolumes=[
                {"mountpoint": mount, "name": name}
                for mount, name in [
                    ("/", "@"),
                    ("/home", "@home"),
                    ("/var/log", "@log"),
                    ("/var/cache/pacman/pkg", "@pkg"),
                ]
            ],
        ),
    ]


def _disk_config(manifest: dict[str, Any]) -> dict[str, Any]:
    return {
        "config_type": "default_layout",
        "device_modifications": [
            {
                "device": "/dev/vda",
                "wipe": True,
                "partitions": _partitions(manifest),
            }
        ],
    }


def _mirror_config() -> dict[str, Any]:
    return {
        "custom_repositories": [],
        "custom_servers": [
            {"url": url}
            for url in [
                "https://mirror.omarchy.org/$repo/os/$arch",
                "https://mirror.rackspace.com/archlinux/$repo/os/$arch",
                "https://geo.mirror.pkgbuild.com/$repo/os/$arch",
            ]
        ],
        "mirror_regions": {},
        "optional_repositories": [],
    }


def _install_config() -> dict[str, Any]:
    return {
        "mode": "full_disk",
        "defer_provisioning": False,
        "target_mount": "/mnt",
        "boot": {
            "esp_mount": "/boot",
            "esp_path": "/EFI/limine",
            "efi_binary": "limine_x64.efi",
            "enable_fallback": True,
        },
        "storage": {"kernel": "linux"},
    }


def _configuration(manifest: dict[str, Any]) -> dict[str, Any]:
    return {
        "app_config": None,
        "archinstall-language": "English",
        "auth_config": {},
        "audio_config": {"audio": "pipewire"},
        "bootloader_config": {
            "bootloader": "Limine",
            "uki": False,
            "removable": False,
        },
        "custom_commands": [],
        "omarchy_install": _install_config(),
        "disk_config": _disk_config(manifest),
        "hostname": manifest["hostname"],
        "kernels": ["linux"],
        "network_config": {"type": "iso"},
        "ntp": True,
        "parallel_downloads": 2,
        "script": None,
        "services": [],
        "swap": True,
        "timezone": "UTC",
        "locale_config": {
            "kb_layout": "us",
            "sys_enc": "UTF-8",
            "sys_lang": "en_US.UTF-8",
        },
        "mirror_config": _mirror_config(),
        "packages": [
            "base-devel",
            "git",
            "omarchy-keyring",
            "omarchy-settings",
            "omarchy",
        ],
        "profile_config": {"gfx_driver": None, "greeter": None, "profile": {}},
        "version": "3.0.9",
    }


def _credentials(manifest: dict[str, Any]) -> dict[str, Any]:
    password = subprocess.check_output(
        ["openssl", "passwd", "-6", "omarchy"],  # ruff: ignore[start-process-with-partial-path] - Host tool.
        text=True,
    ).strip()
    return {
        "root_enc_password": password,
        "users": [
            {
                "enc_password": password,
                "groups": [],
                "sudo": True,
                "username": manifest["user"],
            }
        ],
    }


def _write_seed(
    files: Path, config: dict[str, Any], credentials: dict[str, Any]
) -> None:
    for name, value in {
        "user_configuration.json": config,
        "user_credentials.json": credentials,
    }.items():
        (files / name).write_text(
            json.dumps(value, indent=2) + "\n", encoding="utf-8"
        )


def build(
    directory: str | Path, manifest: dict[str, Any], public_key: str
) -> Path:
    """Build the upstream installer seed from heterogeneous manifest data.

    Returns:
        The FAT image containing the CIDATA configuration.
    """
    directory = Path(directory)
    files = directory / "cidata"
    files.mkdir(exist_ok=True)
    credentials = _credentials(manifest)
    config = _configuration(manifest)
    _write_seed(files, config, credentials)
    for name, value in {
        "user_full_name.txt": "Desktop Test",
        "user_email_address.txt": "desktop@example.invalid",
        "user_encrypt_installation.txt": "false",
        "authorized_keys": public_key,
    }.items():
        (files / name).write_text(value + "\n", encoding="utf-8")
    image = directory / "cidata.img"
    with image.open("wb") as output:
        output.truncate(4 * 1024**2)
    subprocess.run(  # ruff: ignore[subprocess-without-shell-equals-true] - Fixed argv; no shell interpretation.
        ["mkfs.vfat", "-n", "CIDATA", str(image)],  # ruff: ignore[start-process-with-partial-path] - Host tool.
        check=True,
    )
    subprocess.run(  # ruff: ignore[subprocess-without-shell-equals-true] - MCOPY selects a trusted local tool.
        [
            os.environ.get("MCOPY", "mcopy"),
            "-i",
            str(image),
            *map(str, sorted(files.iterdir())),
            "::/",
        ],
        check=True,
    )
    return image
