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
