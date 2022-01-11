import elbus
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
    import msgpack
    print('Call:', event.frame.sender, event.method,
          msgpack.loads(event.get_payload(), raw=False))
    # msgpack reply
    return msgpack.dumps({'ok': True})


name = 'test.client.python.sync.rpc.caller'
# create new elbus client and connect
bus = elbus.client.Client('/tmp/elbus.sock', name)
bus.connect()
# subscribe to all topics
bus.subscribe('#').wait_completed()
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
