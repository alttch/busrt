"use strict";

const ntqdm = require("ntqdm");
const sleep = require("sleep-promise");

const { Bus, Frame, BusOp, QoS } = require("../busrt/");

const onDisconnect = async () => {
  console.log("BUS/RT disconnected");
};

const onFrame = async (frame) => {
  console.log(frame, frame.getPayload().toString());
}

function* generator(steps) {
  let index = 0;
  while (true) {
    yield index;
    index += 1;
  }
}

const test = async () => {
  const bus = new Bus("js");
  bus.onDisconnect = onDisconnect;
  bus.onFrame = onFrame;
  //await bus.connect(("localhost", 9924));
  await bus.connect("/tmp/busrt.sock");
  let op = await bus.subscribe(["#"]);
  console.log(await op.waitCompleted());
  //op = await bus.unsubscribe(["#"]);
  //console.log(await op.waitCompleted());
  //while (bus.isConnected()) {
    //console.log(bus.isConnected());
    //await sleep(1000);
  //}
  //return;
  let iters = 200_000;
  let msg = new Frame(BusOp.Message, QoS.No);
  msg.payload = Buffer.from("hello");
  let start = new Date().getTime() / 1000;
  var tdqm = ntqdm();
  for (let i of ntqdm(generator(iters), { total: iters, logging: true })) {
    let op = await bus.send("y", msg);
    await op.waitCompleted();
  }
  const elapsed = new Date().getTime() / 1000 - start;
  const speed = Math.round(iters / elapsed);
  console.log(speed, "iters/s");
  console.log(Math.round(1_000_000 / speed), "us per iter");
  await bus.disconnect();
};

test();
