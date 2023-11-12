# JavaScript/TypeScript client for BUS/RT

JavaScript/TypeScript client for [BUS/RT](https://github.com/alttch/busrt)

## Client example

```typescript
const { Bus, Frame } = require("busrt");

const onDisconnect = async() => {
  console.log("BUS/RT disconnected");
}

const onFrame = async (frame: Frame) => {
  console.log(frame, frame.getPayload().toString());
}

const main = async () => {
  const bus = new Bus("js");
  bus.onDisconnect = onDisconnect;
  bus.onFrame = onFrame;
  bus.timeout = 5; // seconds, default is 5
  bus.pingInterval = 1; // seconds, default is 1, must be lower than timeout
  await bus.connect("/tmp/busrt.sock"); // local IPC, faster
  // await bus.connect(("localhost", 9924)); // TCP, slower
  console.log(bus.isConnected());

  // subscribe to topics
  const req1 = await bus.subscribe(["some/topic1", "topic2"]);
  await req1.waitCompleted();

  // unsubscribe from topics
  const req2 = await bus.unsubscribe(["some/topic1", "topic2"]);
  await req2.waitCompleted();

  // send one-to-one message
  const req3 = await bus.send("target", Buffer.from("hello"));
  await req3.waitCompleted();

  // disconnect
  await bus.disconnect();
}

main()
```

## RPC example

```javascript
// payloads in this example are packed/unpacked with msgpack
const msgpack = require("msgpackr");
const sleep = require("sleep-promise");
const { Rpc, Bus, RpcEvent, BusError, BusErrorCode, Frame } = require("busrt");

// RPC notification handler
const onNotification = async (ev: RpcEvent) => {
  console.log(ev.frame.primary_sender, ev.getPayload());
}

// RPC call handler
const onCall = async (ev: RpcEvent) => {
  const method = ev.method.toString();
  const payload = ev.getPayload();
  const params = payload.length > 0 ? msgpack.unpack(payload) : null;
  if (method == "test") {
    return msgpack.pack({ ok: true });
  } else if (method == "err") {
    throw new BusError(-777, "test error");
  } else {
    throw new BusError(BusErrorCode.MethodNotFound, Buffer.from(`no such method: ${method}`));
  }
}

// frame handler (broadcasts and topics)
const onFrame = async (frame: Frame) => {
  console.log(frame.primary_sender, msgpack.unpack(frame.getPayload()));
}

const main = async () => {
  // create and connect a new client
  let bus = new Bus("js");
  await bus.connect("/tmp/busrt.sock");
  // init RPC layer
  let rpc = new Rpc(bus);
  // init RPC handlers if incoming event handling is required
  rpc.onNotification = onNotification;
  rpc.onCall = onCall;
  rpc.onFrame = onFrame;
  // send test notification
  await rpc.notify("target", Buffer.from("hello"));
  const payload = { test: 123 };
  // call rpc test method, no response required
  await rpc.call0("target", "test", msgpack.pack(payload));
  // call rpc test method and wait for the response
  try {
    const req = await rpc.call("target", "test", msgpack.pack(payload));
    const result = await req.waitCompleted();
    console.log(msgpack.unpack(result.getPayload().toString()));
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

## API reference

<https://pub.bma.ai/dev/docs/busrt/js/>
