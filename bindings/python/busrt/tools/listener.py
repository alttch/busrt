import sys
sys.path.insert(0, '..')
import busrt
import time
from argparse import ArgumentParser

ap = ArgumentParser()
ap.add_argument('NAME')
ap.add_argument('--rpc', action='store_true')

a = ap.parse_args()


def on_frame(frame):
    print('Frame:', hex(frame.type), frame.sender, frame.topic, frame.payload)


def on_notification(event):
    print('Notification:', event.frame.sender, event.get_payload())


def on_call(event):
    if event.method == b'bmtest':
        return event.get_payload().decode().replace('aaa', 'bbb')
    elif event.method == b'err':
        raise busrt.rpc.RpcException('test error', -777);
    else:
        import msgpack
        print('Call:', event.frame.sender, event.method,
              msgpack.loads(event.get_payload(), raw=False))
        return b'ok'


name = a.NAME
bus = busrt.client.Client('/tmp/busrt.sock', name)
bus.connect()
bus.subscribe('#').wait_completed()
print('listening...')
if a.rpc:
    rpc = busrt.rpc.Rpc(bus)
    rpc.on_frame = on_frame
    rpc.on_notification = on_notification
    rpc.on_call = on_call
    while rpc.is_connected():
        time.sleep(0.1)
else:
    bus.on_frame = on_frame
    while bus.is_connected():
        time.sleep(0.1)
