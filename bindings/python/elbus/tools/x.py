import sys
sys.path.insert(0, '..')
import elbus
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
bus = elbus.client.Client('/tmp/elbus.sock', name)
# bus = elbus.client.Client('localhost:9924', name)
rpc = elbus.rpc.Rpc(bus)
# bus.on_message = on_message
bus.connect()
payload = json.dumps({'hello': True, 'value': 123})
m = elbus.client.Frame(payload)
if target.startswith('='):
    target = target[1:]
    m.type = elbus.client.OP_PUBLISH
elif '*' in target or '?' in target:
    m.type = elbus.client.OP_BROADCAST
print(hex(bus.send(target, m).wait_completed()))
bus.disconnect()
