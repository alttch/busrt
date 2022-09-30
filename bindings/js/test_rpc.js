"use strict";

// payloads will be packed/unpacked as msgpack
const msgpack = require("msgpackr");
const sleep = require("sleep-promise");

const busrt = require("./busrt/src/busrt.js");

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
    throw new busrt.RpcError(-777, "test error");
  } else {
    throw new busrt.RpcError(busrt.RPC_ERROR_CODE_METHOD_NOT_FOUND);
  }
}

// frame handler (broadcasts and topics)
async function on_frame(frame) {
  console.log(frame.sender, frame.get_payload());
}

async function main() {
  // create and connect a new client
  let bus = new busrt.Client("js");
  await bus.connect("/tmp/busrt.sock");
  // init RPC layer
  let rpc = new busrt.Rpc(bus);
  // init RPC handlers if incoming event handling is required
  rpc.on_notification = on_notification;
  rpc.on_call = on_call;
  rpc.on_frame = on_frame;
  // send test notification
  await rpc.notify("target", new busrt.RpcNotification("hello"));
  let payload = { test: 123 };
  // call rpc test method, no response required
  await rpc.call0(
    "target",
    new busrt.RpcRequest("test", msgpack.pack(payload))
  );
  // call rpc test method and wait for the response
  try {
    let request = await rpc.call(
      "target",
      new busrt.RpcRequest("test", msgpack.pack(payload))
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
