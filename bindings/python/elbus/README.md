# Python sync client for elbus

The module contains Python sync client for [elbus](https://elbus.bma.ai/)

## Installation

```shell
pip3 install elbus
```

## Usage examples

### Listener

```python
import elbus
import time

# frame handler
def on_frame(frame):
    print('Frame:', hex(frame.type), frame.sender, frame.topic, frame.payload)

name = 'test.client.python.sync'
# create new elbus client and connect
bus = elbus.client.Client('/tmp/elbus.sock', name)
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
import elbus

name = 'test.client.python.sender'
bus = elbus.client.Client('/tmp/elbus.sock', name)
bus.connect()
# send a regular message
print(
    hex(
        bus.send('test.client.python.sync',
                 elbus.client.Frame('hello')).wait_completed()))
# send a broadcast message
print(
    hex(
        bus.send(
            'test.*',
            elbus.client.Frame('hello everyone',
                               tp=elbus.client.OP_BROADCAST)).wait_completed()))
# publish to a topic with zero QoS (no confirmation required)
bus.send('test/topic',
         elbus.client.Frame('something', tp=elbus.client.OP_PUBLISH, qos=0))
```

## RPC layer

### Handler

```python
import elbus
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
# create new elbus client and connect
bus = elbus.client.Client('/tmp/elbus.sock', name)
bus.connect()
# subscribe to all topics
bus.subscribe('#').wait_completed()
# init rpc
rpc = elbus.rpc.Rpc(bus)
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
import elbus
import msgpack

name = 'test.client.python.sync.rpc.caller'
# create new elbus client and connect
bus = elbus.client.Client('/tmp/elbus.sock', name)
bus.connect()
# init rpc
rpc = elbus.rpc.Rpc(bus)
params = {'hello': 123}
# call a method, no reply required
rpc.call0('test.client.python.sync.rpc',
          elbus.rpc.Request('test', msgpack.dumps(params))).wait_completed()
# call a method and wait for the reply
result = rpc.call('test.client.python.sync.rpc',
                  elbus.rpc.Request('test',
                                    msgpack.dumps(params))).wait_completed()
print(msgpack.loads(result.get_payload(), raw=False))
```
