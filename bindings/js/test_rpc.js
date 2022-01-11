// TODO set buffer size
// TODO timeouts
// TODO package, docs and examples

"use strict";

const msgpack = require("msgpackr");

const elbus = require("./elbus/src/elbus.js");

async function test() {
  let bus = new elbus.Client("js");
  await bus.connect("/tmp/elbus.sock");
  let rpc = new elbus.Rpc(bus);
  await rpc.notify("me", new elbus.RpcNotification("hello"));
  let payload = { test: 123 };
  await rpc.call0("me", new elbus.RpcRequest("test", msgpack.pack(payload)));
  await bus.disconnect();
}

test();
