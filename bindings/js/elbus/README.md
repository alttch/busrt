# javascript client for ELBUS

JavaScript client for [ELBUS](https://elbus.bma.ai/)

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

```javascript
"use strict";

// payloads will be packed/unpacked as msgpack
const msgpack = require("msgpackr");
const sleep = require("sleep-promise");

const elbus = require("elbus");

// RPC notification handler
async function on_notification(ev) {
  console.log(ev.frame.sender, ev.get_payload());
}

// RPC call handler
async function on_call(ev) {
  let method = ev.method.toString();
  if (method == "test") {
    return msgpack.pack({ ok: true });
  } else if (method == "err") {
    throw new elbus.RpcError(-777, "test error");
  } else {
    throw new elbus.RpcError(elbus.RPC_ERROR_CODE_METHOD_NOT_FOUND);
  }
}

// frame handler (broadcasts and topics)
async function on_frame(frame) {
  console.log(frame.sender, frame.get_payload());
}

async function main() {
  // create and connect a new client
  let bus = new elbus.Client("js");
  await bus.connect("/tmp/elbus.sock");
  // init RPC layer
  let rpc = new elbus.Rpc(bus);
  // init RPC handlers if incoming event handling is required
  rpc.on_notification = on_notification;
  rpc.on_call = on_call;
  rpc.on_frame = on_frame;
  // send test notification
  await rpc.notify("target", new elbus.RpcNotification("hello"));
  let payload = { test: 123 };
  // call rpc test method, no response required
  await rpc.call0(
    "target",
    new elbus.RpcRequest("test", msgpack.pack(payload))
  );
  // call rpc test method and wait for the response
  try {
    let request = await rpc.call(
      "target",
      new elbus.RpcRequest("test", msgpack.pack(payload))
    );
    let result = await request.wait_completed();
    console.log(result.get_payload().toString());
  } catch (err) {
    console.log(err.code, err.message.toString());
  }
  // handle local RPC methods while connected
  while (rpc.is_connected()) {
    await sleep(1000);
  }
  await bus.disconnect();
}

main();
```
