import busrt_async
import asyncio
import msgpack


# frame handler (topics/broadcasts)
async def on_frame(frame):
    print('Frame:', hex(frame.type), frame.sender, frame.topic, frame.payload)


# RPC notification handler
async def on_notification(event):
    print('Notification:', event.frame.sender, event.get_payload())


# RPC call handler
async def on_call(event):
    # consider payload is encoded in msgpack
    print('Call:', event.frame.sender, event.method,
          msgpack.loads(event.get_payload(), raw=False))
    # msgpack reply
    return msgpack.dumps({'ok': True})


async def main():
    name = 'test.client.python.async.rpc'
    # create new busrt client and connect
    bus = busrt_async.client.Client('/tmp/busrt.sock', name)
    await bus.connect()
    # subscribe to all topics
    result = await bus.subscribe('#')
    print(hex(await result.wait_completed()))
    # init rpc
    rpc = busrt_async.rpc.Rpc(bus)
    rpc.on_frame = on_frame
    rpc.on_notification = on_notification
    rpc.on_call = on_call
    # wait for frames
    print(f'listening for messages/calls to {name}...')
    while rpc.is_connected():
        await asyncio.sleep(0.1)


asyncio.run(main())
