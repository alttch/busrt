import busrt_async
import asyncio
import time
from argparse import ArgumentParser

ap = ArgumentParser()
ap.add_argument('NAME')
ap.add_argument('--rpc', action='store_true')

a = ap.parse_args()


async def on_frame(frame):
    print('Frame:', hex(frame.type), frame.sender, frame.topic, frame.payload)


async def on_notification(event):
    print('Notification:', event.frame.sender, event.get_payload())


async def on_call(event):
    if event.method == b'bmtest':
        return event.get_payload().decode().replace('aaa', 'bbb')
    else:
        # print('Call:', event.frame.sender, event.method, event.get_payload())
        return b'ok'


async def main():
    name = a.NAME
    bus = busrt_async.client.Client('/tmp/busrt.sock', name)
    await bus.connect()
    op = await bus.subscribe('#')
    await op.wait_completed()
    print('listening...')
    if a.rpc:
        rpc = busrt_async.rpc.Rpc(bus)
        rpc.on_frame = on_frame
        rpc.on_notification = on_notification
        rpc.on_call = on_call
        while rpc.is_connected():
            await asyncio.sleep(0.1)
    else:
        bus.on_frame = on_frame
        while bus.is_connected():
            await asyncio.sleep(0.1)


asyncio.run(main())
time.sleep(1)
