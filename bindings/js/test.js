// TODO set buffer size
// TODO timeouts
// TODO package, docs and examples

"use strict";

const ntqdm = require("ntqdm");
const sleep = require("sleep-promise");

const elbus = require("./elbus/src/elbus.js");

async function disconnected() {
  console.log("elbus disconnected");
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
  let bus = new elbus.Client("js");
  bus.on_disconnect = disconnected;
  bus.on_frame = on_frame;
  await bus.connect("/tmp/elbus.sock");
  let frame = new elbus.Frame(elbus.OP_SUBSCRIBE, 1);
  frame.topic = ["tests", "xxz"];
  //frame.topic = "xxxz";
  let op = await bus.send(frame);
  console.log(await op.wait_completed());
  //while (bus.is_connected()) {
    //console.log(bus.is_connected());
    //await sleep(1000);
  //}
  //return;
  let iters = 100_000;
  let msg = new elbus.Frame(elbus.OP_MESSAGE, 0);
  msg.payload = Buffer.from("hello");
  let start = new Date().getTime() / 1000;
  var tdqm = ntqdm();
  for (let i of ntqdm(generator(iters), { total: iters, logging: true })) {
    let op = await bus.send("y", msg);
    //await op.wait_completed();
  }
  let elapsed = new Date().getTime() / 1000 - start;
  let speed = Math.round(iters / elapsed);
  console.log(speed, "iters/s");
  console.log(Math.round(1_000_000 / speed), "us per iter");
  bus.disconnect();
}

test();
