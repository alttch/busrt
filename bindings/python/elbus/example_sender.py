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
