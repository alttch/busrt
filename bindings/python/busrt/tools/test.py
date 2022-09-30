import sys
sys.path.insert(0, '..')
import busrt

import threading
import time
import random

from tqdm import tqdm

iters = 100_000
workers = 1

cnt = 0


def on_frame(frame):
    global cnt
    cnt += 1


def test(w, iters):
    try:
        global cnt
        payload = b'\x01' * 1024
        name = f'test{random.randint(0,1000_000)}-{w}'
        path = '/tmp/busrt.sock'
        # path = 'localhost:9924'
        bus = busrt.client.Client(path, name)
        bus.on_frame = on_frame
        bus.connect()
        print(f'Connected to {path}')
        for i in range(iters):
            frame = busrt.client.Frame(payload, qos=3)
            b = bus.send('y', frame)
            if not b.wait_completed(timeout=1):
                raise TimeoutError
            cnt += 1
    except Exception as e:
        import traceback
        traceback.print_exc()


started = time.perf_counter()
for w in range(workers):
    threading.Thread(target=test, args=(w, int(iters / workers))).start()
time.sleep(0.1)
with tqdm(total=iters) as pbar:
    prev = 0
    while cnt < iters:
        time.sleep(0.01)
        pbar.update(cnt - prev)
        prev = cnt
pbar.update(iters)
elapsed = time.perf_counter() - started
speed = round(iters / elapsed)
print(f'{round(1_000_000/speed)} us per iter')
