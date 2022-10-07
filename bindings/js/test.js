// TODO set buffer size
// TODO timeouts
// TODO package, docs and examples

"use strict";

const ntqdm = require("ntqdm");
const sleep = require("sleep-promise");

const busrt = require("./busrt/src/busrt.js");

async function disconnected() {
  console.log("bus/rt disconnected");
}

async function on_frame(frame) {
  console.log(frame, frame.get_payload().toString());
}
function* generator(steps) {
  let index = 0;
  while (true) {
    yield index;
    index += 1;
  }
}

async function test() {
  let bus = new busrt.Client("js");
  bus.on_disconnect = disconnected;
  bus.on_frame = on_frame;
  //await bus.connect(("localhost", 9924));
  await bus.connect("/tmp/busrt.sock");
  let op = await bus.subscribe(["tests", "xxz"]);
  console.log(await op.wait_completed());
  op = await bus.unsubscribe(["tests", "xxz"]);
  console.log(await op.wait_completed());
  //while (bus.is_connected()) {
  //console.log(bus.is_connected());
  //await sleep(1000);
  //}
  //return;
  let iters = 200_000;
  let msg = new busrt.Frame(busrt.OP_MESSAGE, 0);
  msg.payload = Buffer.from("hello");
  let start = new Date().getTime() / 1000;
  var tdqm = ntqdm();
  for (let i of ntqdm(generator(iters), { total: iters, logging: true })) {
    let op = await bus.send("y", msg);
    await op.wait_completed();
  }
  let elapsed = new Date().getTime() / 1000 - start;
  let speed = Math.round(iters / elapsed);
  console.log(speed, "iters/s");
  console.log(Math.round(1_000_000 / speed), "us per iter");
  await bus.disconnect();
}

test();
