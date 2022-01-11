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
