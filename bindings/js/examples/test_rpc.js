"use strict";

// payloads will be packed/unpacked as msgpack
const msgpack = require("msgpackr");
const sleep = require("sleep-promise");

const {
  Client,
  Rpc,
  Frame,
  BusOp,
  QoS,
  RpcRequest,
  RpcError,
  RpcErrorCode
} = require("../busrt/");

// RPC notification handler
const onNotification = async (ev) => {
  console.log(ev.getPayload());
};

// RPC call handler
const onCall = async (ev) => {
  const payload = ev.getPayload();
  if (payload.length > 0) {
    console.log(msgpack.unpack(payload));
  }
  const method = ev.method.toString();
  if (method == "test") {
    return msgpack.pack({ ok: true });
  } else if (method == "err") {
    throw new RpcError(-777, "test error");
  } else {
    throw new RpcError(RpcErrorCode.MethodNotFound);
  }
};

// frame handler (broadcasts and topics)
const onFrame = async (frame) => {
  //const payload = frame.getPayload();
  console.log(frame);
  console.log(
    frame.primary_sender,
    frame.topic,
    //payload.length > 0 ? msgpack.unpack(payload) : null
  );
};

const main = async () => {
  // create and connect a new client
  const bus = new Client("js");
  await bus.connect("/opt/eva4/var/bus.ipc");
  // init RPC layer
  const rpc = new Rpc(bus);
  // init RPC handlers if incoming event handling is required
  rpc.onNotification = onNotification;
  rpc.onCall = onCall;
  rpc.onFrame = onFrame;
  await bus.subscribe("test");
  await bus.publish("test", "hello");
  // send test call
  let payload = { i: "#" };
  // call rpc test method, no response required
  //await rpc.call0("target", new RpcRequest("test", msgpack.pack(payload)));
  // call rpc test method and wait for the response
  //try {
    //const request = await rpc.call("eva.core", "test");
    //const result = await request.waitCompleted();
    //console.log(msgpack.unpack(result.getPayload()));
  //} catch (err) {
    //console.log(err.code, err.message.toString());
  //}
  //return;
  // handle local RPC methods while connected
  while (rpc.isConnected()) {
    await sleep(1000);
  }
  await bus.disconnect();
};

main();
