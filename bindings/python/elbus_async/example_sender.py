import elbus_async
import asyncio


async def main():
    name = 'test.client.python.async_sender'
    bus = elbus_async.client.Client('/tmp/elbus.sock', name)
    await bus.connect()
    # send a regular message
    result = await bus.send('test.client.python.async',
                            elbus_async.client.Frame('hello'))
    print(hex(await result.wait_completed()))
    # send a broadcast message
    result = await bus.send(
        'test.*',
        elbus_async.client.Frame('hello everyone',
                                 tp=elbus_async.client.OP_BROADCAST))
    print(hex(await result.wait_completed()))
    # publish to a topic with zero QoS (no confirmation required)
    await bus.send(
        'test/topic',
        elbus_async.client.Frame('something',
                                 tp=elbus_async.client.OP_PUBLISH,
                                 qos=0))


asyncio.run(main())
