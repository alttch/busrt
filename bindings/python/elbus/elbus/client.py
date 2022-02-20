import socket
import threading
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


def on_frame_default(frame):
    pass


class Client:

    def __init__(self, path, name):
        self.path = path
        self.socket = None
        self.buf_size = 8192
        self.name = name
        self.frame_id = 0
        self.ping_interval = 1
        self.on_frame = on_frame_default
        self.socket_lock = threading.Lock()
        self.mgmt_lock = threading.Lock()
        self.connected = False
        self.frames = {}
        self.timeout = 1

    def connect(self):
        with self.mgmt_lock:
            if self.path.endswith('.sock') or self.path.endswith(
                    '.socket') or self.path.endswith(
                        '.ipc') or self.path.startswith('/'):
                self.socket = socket.socket(socket.AF_UNIX)
                path = self.path
            else:
                self.socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
                self.socket.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY,
                                       1)
                path = self.path.rsplit(':', maxsplit=2)
                path[1] = int(path[1])
                path = tuple(path)
            self.socket.setsockopt(socket.SOL_SOCKET, socket.SO_SNDBUF,
                                   self.buf_size)
            self.socket.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF,
                                   self.buf_size)
            self.socket.settimeout(self.timeout)
            self.socket.connect(path)
            buf = self.read_exact(3)
            if buf[0] != GREETINGS:
                raise RuntimeError('Unsupported protocol')
            if int.from_bytes(buf[1:3], 'little') != PROTOCOL_VERSION:
                raise RuntimeError('Unsupported protocol version')
            self.socket.sendall(buf)
            buf = self.socket.recv(1)
            if buf[0] != RESPONSE_OK:
                raise RuntimeError(f'Server response: {hex(buf[0])}')
            name = self.name.encode()
            self.socket.sendall(len(name).to_bytes(2, 'little') + name)
            buf = self.socket.recv(1)
            if buf[0] != RESPONSE_OK:
                raise RuntimeError(f'Server response: {hex(buf[0])}')
            self.connected = True
            threading.Thread(target=self._t_reader, daemon=True).start()
            threading.Thread(target=self._t_pinger, daemon=True).start()

    def _handle_daemon_exception(self):
        with self.mgmt_lock:
            if self.connected:
                try:
                    self.socket.close()
                except:
                    pass
                self.connected = False
                import traceback
                logger.error(traceback.format_exc())

    def _t_pinger(self):
        try:
            while True:
                time.sleep(self.ping_interval)
                with self.socket_lock:
                    self.socket.sendall(PING_FRAME)
        except:
            self._handle_daemon_exception()

    def _t_reader(self):
        try:
            while True:
                buf = self.read_exact(6)
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
                    data_len = int.from_bytes(buf[1:5], 'little')
                    # do not use read_exact for max zero-copy
                    data = b''
                    while len(data) < data_len:
                        buf_size = data_len - len(data)
                        data += self.socket.recv(buf_size if buf_size < self.
                                                 buf_size else self.buf_size)
                    frame = Frame()
                    try:
                        frame.type = buf[0]
                        if buf[0] == OP_PUBLISH:
                            sender, topic, frame.payload = data.split(
                                b'\x00', maxsplit=2)
                            frame.topic = topic.decode()
                        else:
                            sender, frame.payload = data.split(b'\x00',
                                                               maxsplit=1)
                            frame.topic = None
                        frame.sender = sender.decode()
                    except Exception as e:
                        logger.error(f'Invalid frame from the server: {e}')
                        raise
                    try:
                        self.on_frame(frame)
                    except:
                        import traceback
                        logger.error(traceback.format_exc())
        except:
            self._handle_daemon_exception()

    def disconnect(self):
        with self.mgmt_lock:
            self.socket.close()
            self.connected = False

    def read_exact(self, data_len):
        data = b''
        while len(data) < data_len:
            buf_size = data_len - len(data)
            try:
                data += self.socket.recv(
                    buf_size if buf_size < self.buf_size else self.buf_size)
            except socket.timeout:
                if not self.connected:
                    break
        return data

    def send(self, target=None, frame=None):
        try:
            with self.socket_lock:
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
                    self.socket.sendall(
                        frame_id.to_bytes(4, 'little') +
                        flags.to_bytes(1, 'little') +
                        len(payload).to_bytes(4, 'little') + payload)
                else:
                    frame_len = len(target) + len(frame.payload) + 1
                    if frame.header is not None:
                        frame_len += len(frame.header)
                    if frame_len > 0xffff_ffff:
                        raise ValueError('frame too large')
                    self.socket.sendall(
                        frame_id.to_bytes(4, 'little') +
                        flags.to_bytes(1, 'little') +
                        frame_len.to_bytes(4, 'little') + target.encode() +
                        b'\x00' +
                        (frame.header if frame.header is not None else b''))
                    self.socket.sendall(frame.payload.encode(
                    ) if isinstance(frame.payload, str) else frame.payload)
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
            self.completed = threading.Event()

    def is_completed(self):
        if self.qos & 0b1 != 0:
            return self.completed.is_set()
        else:
            return True

    def wait_completed(self, *args, **kwargs):
        if self.qos & 0b1 == 0:
            return RESPONSE_OK
        elif not self.completed.wait(*args, **kwargs):
            raise TimeoutError
        else:
            return self.result


class Frame:

    def __init__(self, payload=None, tp=OP_MESSAGE, qos=0):
        self.payload = payload
        # used for zero-copy
        self.header = None
        self.type = tp
        self.qos = qos
