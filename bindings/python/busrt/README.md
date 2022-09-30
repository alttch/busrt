# Python sync client for BUS/RT

The module contains Python sync client for [BUS/RT](https://github.com/alttch/busrt)

## Installation

```shell
pip3 install busrt
```

## Usage examples

### Listener

```python
import busrt
import time

# frame handler
def on_frame(frame):
    print('Frame:', hex(frame.type), frame.sender, frame.topic, frame.payload)

name = 'test.client.python.sync'
# create new BUS/RT client and connect
bus = busrt.client.Client('/tmp/busrt.sock', name)
bus.on_frame = on_frame
bus.connect()
# subscribe to all topics
bus.subscribe('#').wait_completed()
# wait for frames
print(f'listening for messages to {name}...')
while bus.is_connected():
    time.sleep(0.1)
```

### Sender

```python
import busrt

name = 'test.client.python.sender'
bus = busrt.client.Client('/tmp/busrt.sock', name)
bus.connect()
# send a regular message
print(
    hex(
        bus.send('test.client.python.sync',
                 busrt.client.Frame('hello')).wait_completed()))
# send a broadcast message
print(
    hex(
        bus.send(
            'test.*',
            busrt.client.Frame('hello everyone',
                               tp=busrt.client.OP_BROADCAST)).wait_completed()))
# publish to a topic with zero QoS (no confirmation required)
bus.send('test/topic',
         busrt.client.Frame('something', tp=busrt.client.OP_PUBLISH, qos=0))
```

## RPC layer

### Handler

```python
import busrt
import time
import msgpack

# frame handler (topics/broadcasts)
def on_frame(frame):
    print('Frame:', hex(frame.type), frame.sender, frame.topic, frame.payload)


# RPC notification handler
def on_notification(event):
    print('Notification:', event.frame.sender, event.get_payload())


# RPC call handler
def on_call(event):
    # consider payload is encoded in msgpack
    print('Call:', event.frame.sender, event.method,
          msgpack.loads(event.get_payload(), raw=False))
    # msgpack reply
    return msgpack.dumps({'ok': True})

name = 'test.client.python.sync.rpc'
# create new BUS/RT client and connect
bus = busrt.client.Client('/tmp/busrt.sock', name)
bus.connect()
# subscribe to all topics
bus.subscribe('#').wait_completed()
# init rpc
rpc = busrt.rpc.Rpc(bus)
rpc.on_frame = on_frame
rpc.on_notification = on_notification
rpc.on_call = on_call
# wait for frames
print(f'listening for messages/calls to {name}...')
while rpc.is_connected():
    time.sleep(0.1)
```

### Caller

```python
import busrt
import msgpack

name = 'test.client.python.sync.rpc.caller'
# create new BUS/RT client and connect
bus = busrt.client.Client('/tmp/busrt.sock', name)
bus.connect()
# init rpc
rpc = busrt.rpc.Rpc(bus)
params = {'hello': 123}
# call a method, no reply required
rpc.call0('test.client.python.sync.rpc',
          busrt.rpc.Request('test', msgpack.dumps(params))).wait_completed()
# call a method and wait for the reply
result = rpc.call('test.client.python.sync.rpc',
                  busrt.rpc.Request('test',
                                    msgpack.dumps(params))).wait_completed()
print(msgpack.loads(result.get_payload(), raw=False))
```
