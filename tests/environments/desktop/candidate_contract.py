"""Validate native candidate provenance and plugin compatibility."""  # ruff: ignore[implicit-namespace-package] - Standalone sibling tooling.

from __future__ import annotations

from typing import Any

import build_native

NATIVE_SCHEMA_VERSION = 2


def matching_version(metadata: dict[str, Any], version: str) -> None:
    """Require paired native identities at the package version.

    Raises:
        RuntimeError: Native schema, protocol, role or package version differs.
    """
    if metadata.get("schemaVersion") != NATIVE_SCHEMA_VERSION:
        message = "Candidate requires a component-aware native build schema 2"
        raise RuntimeError(message)
    protocol = metadata.get("controlProtocol")
    if type(protocol) is not int or protocol <= 0:
        message = "Candidate requires a valid native control protocol"
        raise RuntimeError(message)
    components = metadata.get("components")
    if not isinstance(components, dict) or set(components) != {"cli", "daemon"}:
        message = "Candidate requires both CLI and daemon components"
        raise RuntimeError(message)
    for role, expected in build_native.COMPONENTS.items():
        component = components[role]
        if (
            not isinstance(component, dict)
            or component.get("file") != expected["file"]
        ):
            message = f"Candidate has an invalid {role} component"
            raise RuntimeError(message)
        package_id = component.get("cargoArtifact", {}).get("packageId", "")
        if not package_id.endswith(f"#{expected['package']}@{version}"):
            message = (
                f"Package version {version} differs from the selected {role} "
                f"build identity {package_id}; select a matching build"
            )
            raise RuntimeError(message)


def matching_inputs(report: dict[str, Any]) -> None:
    """Require copied native input hashes to match the selected build.

    Raises:
        RuntimeError: A native executable or unit hash differs.
    """
    metadata = report["nativeBuild"]["metadata"]
    expected = {
        item["file"]: item["sha256"] for item in metadata["components"].values()
    }
    expected[build_native.UNIT.name] = metadata["unitSha256"]
    for name, checksum in expected.items():
        if report["nativeInputs"].get(name, {}).get("sha256") != checksum:
            message = f"Candidate native input differs from its build: {name}"
            raise RuntimeError(message)


def matching_protocol(protocol: int, compatibility: dict[str, int]) -> None:
    """Require the plugin to support the selected native protocol.

    Raises:
        RuntimeError: The native protocol falls outside the plugin range.
    """
    if not compatibility["minimum"] <= protocol <= compatibility["maximum"]:
        message = "Candidate plugin is incompatible with the native protocol"
        raise RuntimeError(message)
