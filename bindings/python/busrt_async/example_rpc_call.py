import busrt_async
import msgpack
import asyncio


async def main():
    name = 'test.client.python.async.rpc.caller'
    # create new busrt client and connect
    bus = busrt_async.client.Client('/tmp/busrt.sock', name)
    await bus.connect()
    # init rpc
    rpc = busrt_async.rpc.Rpc(bus)
    params = {'hello': 123}
    # call a method, no reply required
    result = await rpc.call0(
        'test.client.python.async.rpc',
        busrt_async.rpc.Request('test', msgpack.dumps(params)))
    print(hex(await result.wait_completed()))
    # call a method and wait for the reply
    result = await rpc.call(
        'test.client.python.async.rpc',
        busrt_async.rpc.Request('test', msgpack.dumps(params)))
    reply = await result.wait_completed()
    print(msgpack.loads(reply.get_payload(), raw=False))


asyncio.run(main())
