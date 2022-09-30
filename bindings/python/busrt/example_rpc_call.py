import busrt
import msgpack

name = 'test.client.python.sync.rpc.caller'
# create new busrt client and connect
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
