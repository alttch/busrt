"use strict";

// payloads will be packed/unpacked as msgpack
import * as msgpack from "msgpackr";
import sleep from "sleep-promise";

import {
  Bus,
  Rpc,
  Frame,
  QoS,
  BusError,
  BusErrorCode
} from "busrt";

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
    throw new BusError(-777, "test error");
  } else {
    throw new BusError(
      BusErrorCode.RpcMethodNotFound,
      Buffer.from(`no such method: ${method}`)
    );
  }
};

// frame handler (broadcasts and topics)
const onFrame = async (ev) => {
  const payload = ev.frame.getPayload();
  console.log(
    ev.frame.primary_sender,
    ev.frame.topic,
    payload.length > 0 ? msgpack.unpack(payload) : null
  );
};

const main = async () => {
  // create and connect a new client
  const bus = new Bus("js");
  await bus.connect("/opt/eva4/var/bus.ipc");
  const op = await bus.send("test", msgpack.pack("123"));
  try {
    await op.waitCompleted();
  } catch (err) {
    console.log(err.code, err.message?.toString());
  }
  // init RPC layer
  const rpc = new Rpc(bus);
  // init RPC handlers if incoming event handling is required
  rpc.onNotification = onNotification;
  rpc.onCall = onCall;
  rpc.onFrame = onFrame;
  await bus.subscribe("#");
  await bus.publish("test", msgpack.pack("hello"));
  // send test call
  let payload = { i: "#" };
  // call rpc test method, no payload, no response required
  await rpc.call0("target", "test");
  // call rpc test method and wait for the response
  try {
    const request = await rpc.call("eva.core", "test");
    const result = await request.waitCompleted();
    console.log(msgpack.unpack(result.getPayload()));
  } catch (err) {
    console.log(err.code, err.message.toString());
  }
  //return;
  // handle local RPC methods while connected
  while (rpc.isConnected()) {
    await sleep(1000);
  }
  await bus.disconnect();
};

main();
