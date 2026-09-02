#!/usr/bin/python3
"""Pinned Bumble GATT peer used by the isolated Bluetooth radio gate."""

import asyncio
import sys

from bumble.device import Connection, Device
from bumble.gatt import Characteristic, CharacteristicValue, Service
from bumble.transport import open_transport
from typing_extensions import override

SERVICE_UUID = "50DB505C-8AC4-4738-8448-3B1D9CC09CC5"
CHARACTERISTIC_UUID = "D901B45B-4916-412E-ACCA-376ECB603B2C"
BUMBLE_TO_BLUEZ = b"bumble-to-bluez"
BLUEZ_TO_BUMBLE = b"bluez-to-bumble"


class Listener(Device.Listener, Connection.Listener):
    """Report the connection lifecycle required by the test harness."""

    @override
    def on_connection(self, connection: Connection) -> None:
        """Attach the listener to a new fixture connection."""
        connection.listener = self
        print("GATT_CONNECTED", flush=True)

    @override
    def on_disconnection(self, _reason: object) -> None:
        """Report termination of a fixture connection."""
        print("GATT_DISCONNECTED", flush=True)


def read_value(_connection: Connection | None) -> bytes:
    """Return the fixed Bumble-to-BlueZ fixture payload."""
    print(f"GATT_READ {BUMBLE_TO_BLUEZ.hex()}", flush=True)
    return BUMBLE_TO_BLUEZ


def write_value(_connection: Connection | None, value: bytes) -> None:
    """Validate the fixed BlueZ-to-Bumble fixture payload.

    Raises:
        ValueError: If BlueZ writes an unexpected payload.
    """
    if value != BLUEZ_TO_BUMBLE:
        message = f"unexpected GATT write: {value.hex()}"
        raise ValueError(message)
    print(f"GATT_WRITE {value.hex()}", flush=True)


async def main() -> None:
    """Serve the pinned bidirectional GATT fixture until disconnected."""
    async with await open_transport("hci-socket:1") as transport:
        device = Device.from_config_file_with_hci(
            "/bumble/examples/device1.json", transport.source, transport.sink
        )
        device.listener = Listener()
        device.add_service(
            Service(
                SERVICE_UUID,
                [
                    Characteristic(
                        CHARACTERISTIC_UUID,
                        Characteristic.Properties.READ
                        | Characteristic.Properties.WRITE,
                        Characteristic.READABLE | Characteristic.WRITEABLE,
                        CharacteristicValue(read=read_value, write=write_value),
                    )
                ],
            )
        )
        await device.power_on()
        await device.start_advertising(auto_restart=True)
        print("GATT_READY", flush=True)
        await transport.source.terminated


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except Exception as error:
        print(f"GATT_ERROR {error}", file=sys.stderr, flush=True)
        raise
