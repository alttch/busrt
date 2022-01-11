# Python async client for elbus

The module contains Python sync client for [elbus](https://elbus.bma.ai/)

## Installation

```shell
pip3 install elbus_async
```

## Usage examples

### Listener

```python
import elbus_async
import asyncio

# frame handler
async def on_frame(frame):
    print('Frame:', hex(frame.type), frame.sender, frame.topic, frame.payload)


async def main():
    name = 'test.client.python.async'
    # create new elbus client and connect
    bus = elbus_async.client.Client('/tmp/elbus.sock', name)
    bus.on_frame = on_frame
    await bus.connect()
    # subscribe to all topics
    result = await bus.subscribe('#')
    print(hex(await result.wait_completed()))
    # wait for frames
    print(f'listening for messages to {name}...')
    while bus.is_connected():
        await asyncio.sleep(0.1)

asyncio.run(main())
```

### Sender

```python
import elbus_async
import asyncio

async def main():
    name = 'test.client.python.async_sender'
    bus = elbus_async.client.Client('/tmp/elbus.sock', name)
    await bus.connect()
    # send a regular message
    result = await bus.send('test.client.python.async',
                            elbus_async.client.Frame('hello'))
    print(hex(await result.wait_completed()))
    # send a broadcast message
    result = await bus.send(
        'test.*',
        elbus_async.client.Frame('hello everyone',
                                 tp=elbus_async.client.OP_BROADCAST))
    print(hex(await result.wait_completed()))
    # publish to a topic with zero QoS (no confirmation required)
    await bus.send(
        'test/topic',
        elbus_async.client.Frame('something',
                                 tp=elbus_async.client.OP_PUBLISH,
                                 qos=0))

asyncio.run(main())
```

## RPC layer

### Handler

```python
import elbus_async
import asyncio
import msgpack

# frame handler (topics/broadcasts)
async def on_frame(frame):
    print('Frame:', hex(frame.type), frame.sender, frame.topic, frame.payload)

# RPC notification handler
async def on_notification(event):
    print('Notification:', event.frame.sender, event.get_payload())

# RPC call handler
async def on_call(event):
    # consider payload is encoded in msgpack
    print('Call:', event.frame.sender, event.method,
          msgpack.loads(event.get_payload(), raw=False))
    # msgpack reply
    return msgpack.dumps({'ok': True})

async def main():
    name = 'test.client.python.async.rpc'
    # create new elbus client and connect
    bus = elbus_async.client.Client('/tmp/elbus.sock', name)
    await bus.connect()
    # subscribe to all topics
    result = await bus.subscribe('#')
    print(hex(await result.wait_completed()))
    # init rpc
    rpc = elbus_async.rpc.Rpc(bus)
    rpc.on_frame = on_frame
    rpc.on_notification = on_notification
    rpc.on_call = on_call
    # wait for frames
    print(f'listening for messages/calls to {name}...')
    while rpc.is_connected():
        await asyncio.sleep(0.1)

asyncio.run(main())
```

### Caller

```python
import elbus_async
import msgpack
import asyncio

async def main():
    name = 'test.client.python.async.rpc.caller'
    # create new elbus client and connect
    bus = elbus_async.client.Client('/tmp/elbus.sock', name)
    await bus.connect()
    # init rpc
    rpc = elbus_async.rpc.Rpc(bus)
    params = {'hello': 123}
    # call a method, no reply required
    result = await rpc.call0(
        'test.client.python.async.rpc',
        elbus_async.rpc.Request('test', msgpack.dumps(params)))
    print(hex(await result.wait_completed()))
    # call a method and wait for the reply
    result = await rpc.call(
        'test.client.python.async.rpc',
        elbus_async.rpc.Request('test', msgpack.dumps(params)))
    reply = await result.wait_completed()
    print(msgpack.loads(reply.get_payload(), raw=False))

asyncio.run(main())
```
