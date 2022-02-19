import asyncio
import logging

from .client import OP_MESSAGE
from .client import ERR_IO, ERR_TIMEOUT, RESPONSE_OK
from .client import on_frame_default

RPC_NOTIFICATION_HEADER = b'\x00'
RPC_REQUEST_HEADER = b'\x01'
RPC_REPLY_HEADER = b'\x11'
RPC_ERROR_REPLY_HEADER = b'\x12'

RPC_NOTIFICATION = 0x00
RPC_REQUEST = 0x01
RPC_REPLY = 0x11
RPC_ERROR = 0x12

RPC_ERROR_CODE_PARSE = -32700
RPC_ERROR_CODE_INVALID_REQUEST = -32600
RPC_ERROR_CODE_METHOD_NOT_FOUND = -32601
RPC_ERROR_CODE_INVALID_METHOD_PARAMS = -32602
RPC_ERROR_CODE_INTERNAL = -32603

logger = logging.getLogger('elbus.rpc')


async def on_call_default(event):
    raise RpcException('RPC Engine not initialized',
                       RPC_ERROR_CODE_METHOD_NOT_FOUND)


async def on_notification_default(event):
    pass


def format_rpc_e_msg(e):
    if isinstance(e, RpcException):
        return e.rpc_error_payload
    else:
        return str(e)


class RpcException(Exception):

    def __init__(self, msg='', code=RPC_ERROR_CODE_INTERNAL):
        self.rpc_error_code = code
        self.rpc_error_payload = msg
        super().__init__(msg if isinstance(msg, str) else msg.decode())

    def __str__(self):
        return super().__str__() + f' (code: {self.rpc_error_code})'


class Rpc:

    def __init__(self, client):
        self.client = client
        self.client.on_frame = self._handle_frame
        self.call_id = 0
        self.call_lock = asyncio.Lock()
        self.calls = {}
        self.on_frame = on_frame_default
        self.on_call = on_call_default
        self.on_notification = on_notification_default

    def is_connected(self):
        return self.client.connected

    def notify(self, target, notification):
        return self.client.send(target, notification)

    def call0(self, target, request):
        request.header = RPC_REQUEST_HEADER + b'\x00\x00\x00\x00' + \
                request.method + b'\x00'
        return self.client.send(target, request)

    async def call(self, target, request):
        with await self.call_lock:
            call_id = self.call_id + 1
            if call_id == 0xffff_ffff:
                self.call_id = 0
            else:
                self.call_id = call_id
        call_event = RpcCallEvent()
        self.calls[call_id] = call_event
        request.header = RPC_REQUEST_HEADER + call_id.to_bytes(
            4, 'little') + request.method + b'\x00'
        try:
            try:
                code = await (await self.client.send(
                    target,
                    request)).wait_completed(timeout=self.client.timeout)
                if code != RESPONSE_OK:
                    try:
                        del self.calls[call_id]
                    except KeyError:
                        pass
                    err_code = -32000 - code
                    call_event.error = RpcException(f'RPC error', code=err_code)
                    call_event.completed.set()
            except asyncio.TimeoutError:
                try:
                    del self.calls[call_id]
                except KeyError:
                    pass
                err_code = -32000 - ERR_TIMEOUT
                call_event.error = RpcException(f'RPC timeout', code=err_code)
                call_event.completed.set()
        except Exception as e:
            try:
                del self.calls[call_id]
            except KeyError:
                pass
            call_event.error = RpcException(str(e), code=-32000 - ERR_IO)
            call_event.completed.set()
        return call_event

    async def _handle_frame(self, frame):
        try:
            if frame.type == OP_MESSAGE:
                if frame.payload[0] == RPC_NOTIFICATION:
                    event = Event(RPC_NOTIFICATION, frame, 1)
                    await self.on_notification(event)
                elif frame.payload[0] == RPC_REQUEST:
                    sender = frame.sender
                    call_id_b = frame.payload[1:5]
                    call_id = int.from_bytes(call_id_b, 'little')
                    method = frame.payload[5:5 +
                                           frame.payload[5:].index(b'\x00')]
                    event = Event(RPC_REQUEST, frame, 6 + len(method))
                    event.call_id = call_id
                    event.method = method
                    if call_id == 0:
                        await self.on_call(event)
                    else:
                        reply = Reply()
                        try:
                            reply.payload = await self.on_call(event)
                            if reply.payload is None:
                                reply.payload = b''
                            reply.header = RPC_REPLY_HEADER + call_id_b
                        except Exception as e:
                            try:
                                code = e.rpc_error_code
                            except AttributeError:
                                code = RPC_ERROR_CODE_INTERNAL
                            reply.header = (
                                RPC_ERROR_REPLY_HEADER + call_id_b +
                                code.to_bytes(2, 'little', signed=True))
                            reply.payload = format_rpc_e_msg(e)
                        await self.client.send(sender, reply)
                elif frame.payload[0] == RPC_REPLY or frame.payload[
                        0] == RPC_ERROR:
                    call_id = int.from_bytes(frame.payload[1:5], 'little')
                    try:
                        call_event = self.calls.pop(call_id)
                        call_event.frame = frame
                        if frame.payload[0] == RPC_ERROR:
                            err_code = int.from_bytes(frame.payload[5:7],
                                                      'little',
                                                      signed=True)
                            call_event.error = RpcException(frame.payload[7:],
                                                            code=err_code)
                        call_event.completed.set()
                    except KeyError:
                        logger.warning(f'orphaned RPC response: {call_id}')
                else:
                    logger.error(f'Invalid RPC frame code: {frame.payload[0]}')
            else:
                await self.on_frame(frame)
        except Exception as e:
            import traceback
            logger.error(traceback.format_exc())


class RpcCallEvent:

    def __init__(self):
        self.frame = None
        self.error = None
        self.completed = asyncio.Event()

    def is_completed(self):
        return self.completed.is_set()

    async def wait_completed(self, timeout=None):
        if timeout:
            if not await asyncio.wait_for(self.completed.wait(),
                                          timeout=timeout):
                raise TimeoutError
        else:
            await self.completed.wait()
        if self.error is None:
            return self
        else:
            raise self.error

    def get_payload(self):
        return self.frame.payload[5:]

    def is_empty(self):
        return len(self.frame.payload) <= 5


class Event:

    def __init__(self, tp, frame, payload_pos):
        self.tp = tp
        self.frame = frame
        self._payload_pos = payload_pos

    def get_payload(self):
        return self.frame.payload[self._payload_pos:]


class Notification:

    def __init__(self, payload=b''):
        self.payload = payload
        self.type = OP_MESSAGE
        self.qos = 1
        self.header = RPC_NOTIFICATION_HEADER


class Request:

    def __init__(self, method, params=b''):
        self.payload = params
        self.type = OP_MESSAGE
        self.qos = 1
        self.method = method.encode() if isinstance(method, str) else method
        self.header = None


class Reply:

    def __init__(self, result=b''):
        self.payload = result
        self.type = OP_MESSAGE
        self.qos = 1
        self.header = None
