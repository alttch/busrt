import busrt_async
import asyncio


async def main():
    name = 'test.client.python.async_sender'
    bus = busrt_async.client.Client('/tmp/busrt.sock', name)
    await bus.connect()
    # send a regular message
    result = await bus.send('test.client.python.async',
                            busrt_async.client.Frame('hello'))
    print(hex(await result.wait_completed()))
    # send a broadcast message
    result = await bus.send(
        'test.*',
        busrt_async.client.Frame('hello everyone',
                                 tp=busrt_async.client.OP_BROADCAST))
    print(hex(await result.wait_completed()))
    # publish to a topic with zero QoS (no confirmation required)
    await bus.send(
        'test/topic',
        busrt_async.client.Frame('something',
                                 tp=busrt_async.client.OP_PUBLISH,
                                 qos=0))


asyncio.run(main())
