import sys
sys.path.insert(0, '..')
import busrt
import time
from argparse import ArgumentParser

ap = ArgumentParser()
ap.add_argument('NAME')
ap.add_argument('TARGET')
ap.add_argument('MESSAGE')

a = ap.parse_args()

# def on_message(message):
# print(message.type, message.sender, message.topic, message.payload)

name = a.NAME
target = a.TARGET
bus = busrt.client.Client('/tmp/busrt.sock', name)
# bus = busrt.client.Client('localhost:9924', name)
rpc = busrt.rpc.Rpc(bus)
# bus.on_message = on_message
bus.connect()
payload = a.MESSAGE
if payload.startswith(":"):
    request_id = 1
    if ':' in payload[1:]:
        method, params = payload[1:].split(':', maxsplit=1)
    else:
        method = payload[1:]
        params = ''
    request = busrt.rpc.Request(method, params)
    result = rpc.call(target, request).wait_completed().get_payload()
    try:
        import msgpack, json
        data = msgpack.loads(result, raw=False, strict_map_key=False)
        print(json.dumps(data))
    except:
        print(result)
elif payload.startswith('.'):
    notification = busrt.rpc.Notification(payload[1:])
    print(hex(rpc.notify(target, notification).wait_completed()))
else:
    m = busrt.client.Frame(payload)
    if target.startswith('='):
        target = target[1:]
        m.type = busrt.client.OP_PUBLISH
    elif '*' in target or '?' in target:
        m.type = busrt.client.OP_BROADCAST
    print(hex(bus.send(target, m).wait_completed()))
    bus.disconnect()
