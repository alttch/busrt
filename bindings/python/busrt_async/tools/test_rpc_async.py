import busrt_async

import asyncio
import time
import random

iters = 10_000
workers = 1

cnt = 0


async def test(w, iters):
    payload = b'\x01' * 1024
    global cnt
    name = f'test{random.randint(0,1000_000)}-{w}'
    path = '/tmp/busrt.sock'
    # path = 'localhost:9924'
    bus = busrt_async.client.Client(path, name)
    bus.timeout = 5
    bus.buf_size = 1024 * 1024
    await bus.connect()
    rpc = busrt_async.rpc.Rpc(bus)
    print(f'Connected to {path}')
    for i in range(iters):
        result = await rpc.call('y', busrt_async.rpc.Request('test', payload))
        await result.wait_completed()
        cnt += 1
    await bus.disconnect()


async def main():
    started = time.perf_counter()
    for w in range(workers):
        asyncio.ensure_future(test(w, int(iters / workers)))
    while cnt < iters:
        print(cnt)
        await asyncio.sleep(0.1)
    elapsed = time.perf_counter() - started
    speed = round(iters / elapsed)
    print(f'{speed} iters/s ({round(1_000_000/speed)} ms per iter)')


asyncio.run(main())
