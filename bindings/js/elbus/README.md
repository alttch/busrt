# javascript client for elbus

JavaScript client for [elbus](https://elbus.bma.ai/)

## Client example

```javascript
const elbus = require("elbus");

async function on_disconnect() {
  console.log("elbus disconnected");
}

async function on_frame(frame) {
  console.log(frame, frame.get_payload().toString());
}

async function main() {
  // connect
  let bus = new elbus.Client("js");
  bus.on_disconnect = on_disconnect;
  bus.on_frame = on_frame;
  bus.timeout = 5; // seconds, default is 5
  bus.ping_interval = 1; // seconds, default is 1, must be lower than timeout
  await bus.connect("/tmp/elbus.sock"); // local IPC, faster
  // await bus.connect(("localhost", 9924)); // TCP, slower
  console.log(bus.is_connected());

  // subscribe to topics
  let op = bus.subscribe(["some/topic1", "topic2"]); // single topic str or list
  console.log(await op.wait_completed());

  // unsubscribe from topics
  let op = bus.unsubscribe(["some/topic1", "topic2"]);
  console.log(await op.wait_completed());

  // send one-to-one message
  let msg = new elbus.Frame(elbus.OP_MESSAGE, 1);
  msg.payload = Buffer.from("hello");
  let op = await bus.send("target", msg);
  console.log(await op.wait_completed());

  // disconnect
  await bus.disconnect();
}

main()
```

## RPC example

coming soon
