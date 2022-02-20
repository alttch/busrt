import socket
import asyncio
import logging
import time

logger = logging.getLogger('elbus.client')

GREETINGS = 0xEB

PROTOCOL_VERSION = 1

OP_NOP = 0
OP_PUBLISH = 1
OP_SUBSCRIBE = 2
OP_UNSUBSCRIBE = 3
OP_MESSAGE = 0x12
OP_BROADCAST = 0x13
OP_ACK = 0xFE

RESPONSE_OK = 0x01
ERR_CLIENT_NOT_REGISTERED = 0x71
ERR_DATA = 0x72
ERR_IO = 0x73
ERR_OTHER = 0x74
ERR_NOT_SUPPORTED = 0x75
ERR_BUSY = 0x76
ERR_NOT_DELIVERED = 0x77
ERR_TIMEOUT = 0x78

PING_FRAME = b'\x00' * 9


async def on_frame_default(frame):
    pass


class Client:

    def __init__(self, path, name):
        self.path = path
        self.writer = None
        self.reader_fut = None
        self.pinger_fut = None
        self.buf_size = 8192
        self.name = name
        self.frame_id = 0
        self.ping_interval = 1
        self.on_frame = on_frame_default
        self.socket_lock = asyncio.Lock()
        self.mgmt_lock = asyncio.Lock()
        self.connected = False
        self.frames = {}
        self.timeout = 1

    async def connect(self):
        with await self.mgmt_lock:
            if self.path.endswith('.sock') or self.path.endswith(
                    '.socket') or self.path.endswith(
                        '.ipc') or self.path.startswith('/'):
                reader, writer = await asyncio.open_unix_connection(
                    self.path, limit=self.buf_size)
            else:
                host, port = self.path.rsplit(':', maxsplit=2)
                reader, writer = await asyncio.open_connection(
                    host, int(port), limit=self.buf_size)
            buf = await asyncio.wait_for(self.readexactly(reader, 3),
                                         timeout=self.timeout)
            if buf[0] != GREETINGS:
                raise RuntimeError('Unsupported protocol')
            if int.from_bytes(buf[1:3], 'little') != PROTOCOL_VERSION:
                raise RuntimeError('Unsupported protocol version')
            writer.write(buf)
            await asyncio.wait_for(writer.drain(), timeout=self.timeout)
            buf = await asyncio.wait_for(self.readexactly(reader, 1),
                                         timeout=self.timeout)
            if buf[0] != RESPONSE_OK:
                raise RuntimeError(f'Server response: {hex(buf[0])}')
            name = self.name.encode()
            writer.write(len(name).to_bytes(2, 'little') + name)
            await asyncio.wait_for(writer.drain(), timeout=self.timeout)
            buf = await asyncio.wait_for(self.readexactly(reader, 1),
                                         timeout=self.timeout)
            if buf[0] != RESPONSE_OK:
                raise RuntimeError(f'Server response: {hex(buf[0])}')
            self.writer = writer
            self.connected = True
            self.pinger_fut = asyncio.ensure_future(self._t_pinger())
            self.reader_fut = asyncio.ensure_future(self._t_reader(reader))

    async def handle_daemon_exception(self, e):
        with await self.mgmt_lock:
            if self.connected:
                await self._disconnect()
                import traceback
                logger.error(traceback.format_exc())

    async def _t_pinger(self):
        try:
            while True:
                await asyncio.sleep(self.ping_interval)
                with await self.socket_lock:
                    self.writer.write(PING_FRAME)
                    await asyncio.wait_for(self.writer.drain(),
                                           timeout=self.timeout)
        except Exception as e:
            asyncio.ensure_future(self.handle_daemon_exception(e))

    async def _t_reader(self, reader):
        try:
            while True:
                buf = await self.readexactly(reader, 6)
                if buf[0] == OP_NOP:
                    continue
                elif buf[0] == OP_ACK:
                    op_id = int.from_bytes(buf[1:5], 'little')
                    try:
                        o = self.frames.pop(op_id)
                        o.result = buf[5]
                        o.completed.set()
                    except KeyError:
                        logger.warning(f'orphaned elbus frame ack {op_id}')
                else:

                    async def read_frame(tp, data_len):
                        frame = Frame()
                        frame.type = tp
                        sender = await reader.readuntil(b'\x00')
                        data_len -= len(sender)
                        frame.sender = sender[:-1].decode()
                        if buf[0] == OP_PUBLISH:
                            topic = await reader.readuntil(b'\x00')
                            data_len -= len(topic)
                            frame.topic = topic[:-1].decode()
                        else:
                            frame.topic = None
                        data = b''
                        while len(data) < data_len:
                            buf_size = data_len - len(data)
                            prev_len = len(data)
                            try:
                                data += await reader.readexactly(
                                    buf_size if buf_size < self.buf_size else
                                    self.buf_size)
                            except asyncio.IncompleteReadError:
                                pass
                        frame.payload = data
                        return frame

                    try:
                        data_len = int.from_bytes(buf[1:5], 'little')
                        frame = await read_frame(buf[0], data_len)
                    except Exception as e:
                        logger.error(f'Invalid frame from the server: {e}')
                        raise
                    asyncio.ensure_future(self.on_frame(frame))
        except Exception as e:
            asyncio.ensure_future(self.handle_daemon_exception(e))

    async def readexactly(self, reader, data_len):
        data = b''
        while len(data) < data_len:
            buf_size = data_len - len(data)
            try:
                chunk = await reader.readexactly(
                    buf_size if buf_size < self.buf_size else self.buf_size)
                data += chunk
            except asyncio.IncompleteReadError:
                await asyncio.sleep(0.01)
        return data

    async def disconnect(self):
        with await self.mgmt_lock:
            await self._disconnect()

    async def _disconnect(self):
        self.connected = False
        self.writer.close()
        if self.reader_fut is not None:
            self.reader_fut.cancel()
        if self.pinger_fut is not None:
            self.pinger_fut.cancel()

    async def send(self, target=None, frame=None):
        try:
            with await self.socket_lock:
                self.frame_id += 1
                if self.frame_id > 0xffff_ffff:
                    self.frame_id = 1
                frame_id = self.frame_id
                o = ClientFrame(frame.qos)
                if frame.qos & 0b1 != 0:
                    self.frames[frame_id] = o
                flags = frame.type | frame.qos << 6
                if frame.type == OP_SUBSCRIBE or frame.type == OP_UNSUBSCRIBE:
                    topics = frame.topic if isinstance(frame.topic,
                                                       list) else [frame.topic]
                    payload = b'\x00'.join(t.encode() for t in topics)
                    self.writer.write(
                        frame_id.to_bytes(4, 'little') +
                        flags.to_bytes(1, 'little') +
                        len(payload).to_bytes(4, 'little') + payload)
                else:
                    frame_len = len(target) + len(frame.payload) + 1
                    if frame.header is not None:
                        frame_len += len(frame.header)
                    if frame_len > 0xffff_ffff:
                        raise ValueError('frame too large')
                    self.writer.write(
                        frame_id.to_bytes(4, 'little') +
                        flags.to_bytes(1, 'little') +
                        frame_len.to_bytes(4, 'little') + target.encode() +
                        b'\x00' +
                        (frame.header if frame.header is not None else b''))
                    self.writer.write(frame.payload.encode(
                    ) if isinstance(frame.payload, str) else frame.payload)
                await self.writer.drain()
                return o
        except:
            try:
                del self.frames[frame_id]
            except KeyError:
                pass
            raise

    def subscribe(self, topics):
        frame = Frame(tp=OP_SUBSCRIBE)
        frame.topic = topics
        return self.send(None, frame)

    def unsubscribe(self, topics):
        frame = Frame(tp=OP_UNSUBSCRIBE)
        frame.topic = topics
        return self.send(None, frame)

    def is_connected(self):
        return self.connected


class ClientFrame:

    def __init__(self, qos):
        self.qos = qos
        self.result = 0
        if qos & 0b1 != 0:
            self.completed = asyncio.Event()
        else:
            self.completed = None

    def is_completed(self):
        if self.qos & 0b1 != 0:
            return self.completed.is_set()
        else:
            return True

    async def wait_completed(self, timeout=None):
        if self.qos & 0b1 == 0:
            return RESPONSE_OK
        elif timeout:
            await asyncio.wait_for(self.completed.wait(), timeout=timeout)
        else:
            await self.completed.wait()
        return self.result


class Frame:

    def __init__(self, payload=None, tp=OP_MESSAGE, qos=0):
        self.payload = payload
        # used for zero-copy
        self.header = None
        self.type = tp
        self.qos = qos
