import busrt

import threading
import time
import random

iters = 10_000
workers = 1

cnt = 0


def test(w, iters):
    payload = b'\x01' * 1024
    global cnt
    name = f'test{random.randint(0,1000_000)}-{w}'
    path = '/tmp/busrt.sock'
    # path = 'localhost:9924'
    bus = busrt.client.Client(path, name)
    bus.timeout = 5
    bus.buf_size = 1024 * 1024
    bus.connect()
    rpc = busrt.rpc.Rpc(bus)
    print(f'Connected to {path}')
    for i in range(iters):
        result = rpc.call('y', busrt.rpc.Request('test',
                                         payload)).wait_completed(timeout=5)
        cnt += 1


started = time.perf_counter()
for w in range(workers):
    threading.Thread(target=test, args=(w, int(iters / workers))).start()
while cnt < iters:
    print(cnt)
    time.sleep(0.1)
elapsed = time.perf_counter() - started
speed = round(iters / elapsed)
print(f'{speed} iters/s ({round(1_000_000/speed)} ms per iter)')
