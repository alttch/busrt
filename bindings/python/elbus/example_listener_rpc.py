import elbus
import time


# frame handler (topics/broadcasts)
def on_frame(frame):
    print('Frame:', hex(frame.type), frame.sender, frame.topic, frame.payload)


# RPC notification handler
def on_notification(event):
    print('Notification:', event.frame.sender, event.get_payload())


# RPC call handler
def on_call(event):
    # consider payload is encoded in msgpack
    import msgpack
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
