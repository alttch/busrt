# javascript client for elbus

JavaScript client for [elbus](https://elbus.bma.ai/)

Client example:

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
  await bus.connect("/tmp/elbus.sock");

  // subscribe to topics
  let frame = new elbus.Frame(elbus.OP_SUBSCRIBE, 1);
  frame.topic = ["some/topic1", "topic2"];
  let op = await bus.send(frame);
  console.log(await op.wait_completed());

  // send one-to-one message
  let msg = new elbus.Frame(elbus.OP_MESSAGE, 1);
  msg.payload = Buffer.from("hello");
  let op = await bus.send("target", msg);
  console.log(await op.wait_completed());

  // disconnect
  bus.disconnect();
}

main()
```
