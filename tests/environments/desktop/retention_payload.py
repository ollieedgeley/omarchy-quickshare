"""Exact-file retention checks shared by host and guest Python."""  # ruff: ignore[implicit-namespace-package] - Standalone tooling imports this helper.

import hashlib
import json
import os
import stat
import sys
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path
from typing import Any, BinaryIO


@contextmanager
def directory(path: Path) -> Iterator[int]:
    """Open directory components without following symlinks.

    Yields:
        The opened directory descriptor.

    Raises:
        ValueError: The supplied path is not absolute or traverses parents.
    """
    path = Path(path)
    if not path.is_absolute() or ".." in path.parts:
        message = "An absolute, non-traversing path is required"
        raise ValueError(message)
    descriptor = os.open("/", os.O_RDONLY | os.O_DIRECTORY)
    try:
        for part in path.parts[1:]:
            child = os.open(
                part,
                os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                dir_fd=descriptor,
            )
            os.close(descriptor)
            descriptor = child
        yield descriptor
    finally:
        os.close(descriptor)


def _identity(value: os.stat_result) -> tuple[int, ...]:
    return (
        value.st_dev,
        value.st_ino,
        value.st_size,
        value.st_nlink,
        value.st_mode,
        value.st_uid,
        value.st_gid,
        value.st_mtime_ns,
        value.st_ctime_ns,
    )


def _verify(source: BinaryIO, expected: dict[str, Any]) -> dict[str, Any]:
    before = os.fstat(source.fileno())
    if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1:
        message = "Not a single-link regular file"
        raise ValueError(message)
    if "sizeBytes" in expected and before.st_size != expected["sizeBytes"]:
        message = "File size differs from recorded synthetic bytes"
        raise ValueError(message)
    digest = hashlib.file_digest(source, "sha256").hexdigest()
    if digest != expected["sha256"]:
        message = "SHA-256 differs from recorded synthetic bytes"
        raise ValueError(message)
    return {
        "logicalBytes": before.st_size,
        "allocatedFileBytes": before.st_blocks * 512,
        "identity": list(_identity(before)),
        "sha256": digest,
        "status": "eligible",
        "reason": "Exact recorded payload bytes verified",
    }


@contextmanager
def _verified_payload(
    path: Path, expected: dict[str, Any]
) -> Iterator[tuple[int, dict[str, Any]]]:
    with directory(path.parent) as parent:
        descriptor = os.open(
            path.name,
            os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK,
            dir_fd=parent,
        )
        with os.fdopen(descriptor, "rb") as source:
            item = _verify(source, expected)
            after = os.stat(path.name, dir_fd=parent, follow_symlinks=False)
            if item["identity"] != list(_identity(after)):
                message = "File changed during byte verification"
                raise ValueError(message)
            yield parent, item


def payload(
    path: Path, expected: dict[str, Any], *, apply: bool = False
) -> dict[str, Any]:
    """Inspect or unlink one single-link regular file with recorded bytes.

    Returns:
        Exact path, byte accounting and eligibility or removal outcome.
    """
    path = Path(path)
    item = {"path": str(path), "status": "protected"}
    try:
        with _verified_payload(path, expected) as (parent, verified):
            item.update(verified)
            if apply:
                os.unlink(path.name, dir_fd=parent)
                item["status"] = "removed"
    except FileNotFoundError:
        item.update(
            status="absent", reason="Exact owned path is already absent"
        )
    except (OSError, ValueError) as error:
        item.update(status="protected", reason=str(error))
    return item


if __name__ == "__main__":
    request = json.loads(sys.argv[1])
    path = Path.home() / request["path"]
    print(
        json.dumps(payload(path, request["expected"], apply=request["apply"]))
    )
