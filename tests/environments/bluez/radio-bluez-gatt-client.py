#!/usr/bin/python3
"""Exercise a Bumble GATT peer through real BlueZ D-Bus APIs."""

import time

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


def wait_for(description, lookup, timeout=10):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = lookup()
        if value is not None:
            return value
        time.sleep(0.05)
    raise TimeoutError(f"timed out waiting for {description}")


def managed_objects(manager):
    return manager.GetManagedObjects()


def interface_path(manager, interface, predicate=lambda _props: True):
    for path, interfaces in managed_objects(manager).items():
        properties = interfaces.get(interface)
        if properties is not None and predicate(properties):
            return str(path)
    return None


def main():
    bus = dbus.SystemBus()
    manager = dbus.Interface(bus.get_object(BLUEZ, "/"), OBJECT_MANAGER)
    adapter_path = wait_for(
        "hci0 adapter",
        lambda: interface_path(
            manager,
            ADAPTER,
            lambda props: str(props.get("Address", "")) != PEER_ADDRESS,
        ),
    )
    adapter_object = bus.get_object(BLUEZ, adapter_path)
    adapter_properties = dbus.Interface(adapter_object, PROPERTIES)
    adapter_properties.Set(ADAPTER, "Powered", dbus.Boolean(True))
    adapter = dbus.Interface(adapter_object, ADAPTER)
    adapter.SetDiscoveryFilter({"Transport": dbus.String("le")})
    adapter.StartDiscovery()
    try:
        device_path = wait_for(
            "Bumble advertisement",
            lambda: interface_path(
                manager,
                DEVICE,
                lambda props: str(props.get("Address", "")) == PEER_ADDRESS,
            ),
        )
    finally:
        adapter.StopDiscovery()

    device_object = bus.get_object(BLUEZ, device_path)
    device = dbus.Interface(device_object, DEVICE)
    device.Connect()
    try:
        characteristic_path = wait_for(
            "test GATT characteristic",
            lambda: interface_path(
                manager,
                CHARACTERISTIC,
                lambda props: str(props.get("UUID", "")).lower()
                == CHARACTERISTIC_UUID,
            ),
        )
        characteristic = dbus.Interface(
            bus.get_object(BLUEZ, characteristic_path), CHARACTERISTIC
        )
        received = bytes(characteristic.ReadValue({}))
        if received != BUMBLE_TO_BLUEZ:
            raise AssertionError(f"unexpected GATT read: {received!r}")
        characteristic.WriteValue(
            dbus.Array(BLUEZ_TO_BUMBLE, signature="y"), {}
        )
    finally:
        device.Disconnect()

    print("BLUEZ_GATT_BIDIRECTIONAL_OK", flush=True)


if __name__ == "__main__":
    main()
