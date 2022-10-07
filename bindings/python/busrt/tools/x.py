import sys
sys.path.insert(0, '..')
import busrt
import time
import msgpack
import json
from argparse import ArgumentParser

ap = ArgumentParser()
ap.add_argument('NAME')
ap.add_argument('TARGET')

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
payload = json.dumps({'hello': True, 'value': 123})
m = busrt.client.Frame(payload)
if target.startswith('='):
    target = target[1:]
    m.type = busrt.client.OP_PUBLISH
elif '*' in target or '?' in target:
    m.type = busrt.client.OP_BROADCAST
print(hex(bus.send(target, m).wait_completed()))
bus.disconnect()
