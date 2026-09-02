#!/usr/bin/python3
"""Exercise a Bumble GATT peer through real BlueZ D-Bus APIs."""

import time
from collections.abc import Callable

import dbus

BLUEZ = "org.bluez"
PROPERTIES = "org.freedesktop.DBus.Properties"
OBJECT_MANAGER = "org.freedesktop.DBus.ObjectManager"
ADAPTER = "org.bluez.Adapter1"
DEVICE = "org.bluez.Device1"
CHARACTERISTIC = "org.bluez.GattCharacteristic1"
PEER_ADDRESS = "F0:F1:F2:F3:F4:F5"
CHARACTERISTIC_UUID = "d901b45b-4916-412e-acca-376ecb603b2c"
BUMBLE_TO_BLUEZ = b"bumble-to-bluez"
BLUEZ_TO_BUMBLE = b"bluez-to-bumble"


def wait_for(
    description: str,
    lookup: Callable[[], object | None],
    timeout: float = 10,
) -> object:
    """Poll a BlueZ observation until it exists or the budget expires.

    Returns:
        The first available observation.

    Raises:
        TimeoutError: If the observation does not arrive within the budget.
    """
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = lookup()
        if value is not None:
            return value
        time.sleep(0.05)
    message = f"timed out waiting for {description}"
    raise TimeoutError(message)


def managed_objects(manager: object) -> dict[object, dict[str, object]]:
    """Read BlueZ's managed-object snapshot.

    Returns:
        The current D-Bus object and interface map.
    """
    return manager.GetManagedObjects()


def interface_path(
    manager: object,
    interface: str,
    predicate: Callable[[object], bool] = lambda _props: True,
) -> str | None:
    """Find the first managed object matching an interface predicate.

    Returns:
        The matching D-Bus object path, when one exists.
    """
    for path, interfaces in managed_objects(manager).items():
        properties = interfaces.get(interface)
        if properties is not None and predicate(properties):
            return str(path)
    return None


def configure_adapter(bus: object, manager: object) -> object:
    """Power the local adapter and constrain discovery to BLE.

    Returns:
        The configured BlueZ adapter interface.
    """
    path = wait_for(
        "hci0 adapter",
        lambda: interface_path(
            manager,
            ADAPTER,
            lambda props: str(props.get("Address", "")) != PEER_ADDRESS,
        ),
    )
    adapter_object = bus.get_object(BLUEZ, path)
    adapter_properties = dbus.Interface(adapter_object, PROPERTIES)
    adapter_properties.Set(ADAPTER, "Powered", dbus.Boolean(1))
    adapter = dbus.Interface(adapter_object, ADAPTER)
    adapter.SetDiscoveryFilter({"Transport": dbus.String("le")})
    return adapter


def discover_peer(manager: object, adapter: object) -> object:
    """Discover the fixed Bumble peer and return its object path.

    Returns:
        The discovered peer's D-Bus object path.
    """
    adapter.StartDiscovery()
    try:
        return wait_for(
            "Bumble advertisement",
            lambda: interface_path(
                manager,
                DEVICE,
                lambda props: str(props.get("Address", "")) == PEER_ADDRESS,
            ),
        )
    finally:
        adapter.StopDiscovery()


def exchange_gatt_value(
    bus: object,
    manager: object,
    device_path: object,
) -> None:
    """Read and write the fixture characteristic through BlueZ.

    Raises:
        AssertionError: If the fixture read differs from the expected payload.
    """
    device_object = bus.get_object(BLUEZ, device_path)
    device = dbus.Interface(device_object, DEVICE)
    device.Connect()
    try:
        characteristic_path = wait_for(
            "test GATT characteristic",
            lambda: interface_path(
                manager,
                CHARACTERISTIC,
                lambda props: (
                    str(props.get("UUID", "")).lower() == CHARACTERISTIC_UUID
                ),
            ),
        )
        characteristic = dbus.Interface(
            bus.get_object(BLUEZ, characteristic_path), CHARACTERISTIC
        )
        received = bytes(characteristic.ReadValue({}))
        if received != BUMBLE_TO_BLUEZ:
            message = f"unexpected GATT read: {received!r}"
            raise AssertionError(message)
        characteristic.WriteValue(
            dbus.Array(BLUEZ_TO_BUMBLE, signature="y"), {}
        )
    finally:
        device.Disconnect()


def main() -> None:
    """Run the bidirectional BlueZ-to-Bumble GATT proof."""
    bus = dbus.SystemBus()
    manager = dbus.Interface(bus.get_object(BLUEZ, "/"), OBJECT_MANAGER)
    adapter = configure_adapter(bus, manager)
    device_path = discover_peer(manager, adapter)
    exchange_gatt_value(bus, manager, device_path)
    print("BLUEZ_GATT_BIDIRECTIONAL_OK", flush=True)


if __name__ == "__main__":
    main()
