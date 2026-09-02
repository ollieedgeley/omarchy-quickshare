"""Relay complete H4 frames between the guest socket and Bluetooth server."""

import asyncio
import stat
import struct
from pathlib import Path

LISTENER = "/runtime/h4-relay.sock"
UPSTREAM = "/runtime/bt-server-bredrle"


async def _frame(reader: asyncio.StreamReader) -> bytes:
    kind = await reader.readexactly(1)
    if kind == b"\x01":
        header = await reader.readexactly(3)
        length = header[2]
    elif kind == b"\x02":
        header = await reader.readexactly(4)
        length = struct.unpack_from("<H", header, 2)[0]
    elif kind == b"\x03":
        header = await reader.readexactly(3)
        length = header[2]
    elif kind == b"\x04":
        header = await reader.readexactly(2)
        length = header[1]
    elif kind == b"\x05":
        header = await reader.readexactly(4)
        length = struct.unpack_from("<H", header, 2)[0] & 0x3FFF
    else:
        message = f"unsupported H4 packet type {kind.hex()}"
        raise ValueError(message)
    return kind + header + await reader.readexactly(length)


async def _copy_frames(
    reader: asyncio.StreamReader, writer: asyncio.StreamWriter
) -> None:
    try:
        while True:
            writer.write(await _frame(reader))
            await writer.drain()
    except asyncio.IncompleteReadError:
        writer.close()
        await writer.wait_closed()


async def _relay(
    client_reader: asyncio.StreamReader, client_writer: asyncio.StreamWriter
) -> None:
    upstream_reader, upstream_writer = await asyncio.open_unix_connection(
        UPSTREAM
    )
    await asyncio.gather(
        _copy_frames(client_reader, upstream_writer),
        _copy_frames(upstream_reader, client_writer),
    )


def _prepare_listener() -> None:
    listener = Path(LISTENER)
    if listener.exists():
        listener.unlink()


def _set_listener_mode() -> None:
    Path(LISTENER).chmod(
        stat.S_IRUSR | stat.S_IWUSR | stat.S_IRGRP | stat.S_IWGRP
    )


async def _main() -> None:
    _prepare_listener()
    server = await asyncio.start_unix_server(_relay, path=LISTENER)
    _set_listener_mode()
    async with server:
        await server.serve_forever()


asyncio.run(_main())
