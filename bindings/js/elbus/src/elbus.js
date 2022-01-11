"use strict";

const GREETINGS = 0xeb;
const PROTOCOL_VERSION = 1;
const RESPONSE_OK = 1;
const PING_FRAME = Buffer.from([0, 0, 0, 0, 0, 0, 0, 0, 0]);

const OP_NOP = 0;
const OP_PUBLISH = 1;
const OP_SUBSCRIBE = 2;
const OP_UNSUBSCRIBE = 3;
const OP_MESSAGE = 0x12;
const OP_BROADCAST = 0x13;
const OP_ACK = 0xfe;

const ERR_CLIENT_NOT_REGISTERED = 0x71;
const ERR_DATA = 0x72;
const ERR_IO = 0x73;
const ERR_OTHER = 0x74;
const ERR_NOT_SUPPORTED = 0x75;
const ERR_BUSY = 0x76;
const ERR_NOT_DELIVERED = 0x77;
const ERR_TIMEOUT = 0x78;

const RPC_NOTIFICATION = 0x00;
const RPC_REQUEST = 0x01;
const RPC_REPLY = 0x11;
const RPC_ERROR = 0x12;

const net = require("net");
const sleep = require("sleep-promise");
const Mutex = require("async-mutex").Mutex;
const { PromiseSocket, TimeoutError } = require("promise-socket");

class Frame {
  constructor(type, qos) {
    if (type !== undefined) {
      this.type = type;
    } else {
      this.type = OP_MESSAGE;
    }
    if (qos !== undefined) {
      this.qos = qos;
    } else {
      this.qos = 1;
    }
  }
  get_payload() {
    return this.buf.slice(this.payload_pos);
  }
}

class ClientFrame {
  constructor(qos) {
    this.qos = qos;
    if (qos > 0) {
      this.lock = new Mutex();
    }
  }
  is_completed() {
    if (this.qos > 0) {
      return this.result !== undefined;
    } else {
      return true;
    }
  }
  async lock_frame() {
    this.release = await this.lock.acquire();
  }
  async wait_completed() {
    if (this.qos == 0) {
      return RESPONSE_OK;
    }
    let r = await this.lock.acquire();
    r();
    return this.result;
  }
}

class RpcCallEvent {
  constructor() {
    this.completed = new Mutex();
  }
  is_completed() {
    return this.frame !== undefined || this.error !== undefined;
  }
  async lock_event() {
    this.release = await this.lock.acquire();
  }
  async wait_completed() {
    let r = await self.lock.acquire();
    r();
    if (this.error) {
      throw this.error;
    } else {
      return this;
    }
  }
  get_payload() {
    return this.frame.payload.slice(5);
  }
}

class RpcEvent {
  constructor(tp, frame, payload_pos) {
    this.tp = tp;
    this.frame = frame;
    this.payload_pos = payload_pos;
  }
  get_payload() {
    return this.frame.payload.slice(this.payload_pos);
  }
}

class RpcNotification {
  constructor(payload) {
    this.payload = payload;
    this.qos = 1;
    this.header = Buffer.from([RPC_NOTIFICATION]);
    this.type = OP_MESSAGE;
  }
}

class RpcRequest {
  constructor(method, params) {
    this.payload = params;
    this.qos = 1;
    this.method = Buffer.from(method, "utf8");
    this.type = OP_MESSAGE;
  }
}

class RpcReply {
  constructor(method, result) {
    this.payload = result;
    this.qos = 1;
    this.method = Buffer.from(method, "utf8");
    this.type = OP_MESSAGE;
  }
}

class Rpc {
  constructor(client) {
    this.client = client;
    this.client.on_frame = this._handle_frame;
    this.call_id = 0;
    this.call_lock = new Mutex();
    this.calls = {};
  }
  is_connected() {
    return this.client.connected;
  }
  async notify(target, notification) {
    return await this.client.send(target, notification);
  }
  async call0(target, request) {
    request.header = Buffer.concat([
      Buffer.from([RPC_REQUEST, 0, 0, 0, 0]),
      request.method,
      Buffer.alloc(1)
    ]);
    return await this.client.send(target, request);
  }
  // TODO call
  // TODO handle_frame
}

class Client {
  constructor(name) {
    if (name === undefined) {
      throw "name is not defined";
    }
    this.name = name;
    this.ping_interval = 1;
    this.timeout = 5;
    this.connected = false;
    this.mgmt_lock = new Mutex();
    this.socket_lock = new Mutex();
    this.on_frame = null;
    this.on_disconnect = null;
    this.frame_id = 0;
    this.frames = {};
  }
  async connect(path) {
    let release = await this.mgmt_lock.acquire();
    try {
      let sock = new net.Socket();
      sock.setNoDelay(true);
      this.socket = new PromiseSocket(sock);
      this.socket.setTimeout(this.timeout * 1000);
      await this.socket.connect(path);
      let header = await this.socket.read(3);
      if (header[0] != GREETINGS) {
        throw "Unsupported protocol";
      }
      let ver = Buffer.from(header.slice(1, 3));
      if (ver.readInt16LE(0) != PROTOCOL_VERSION) {
        throw "Unsupported protocol version";
      }
      await this.socket.writeAll(header);
      let code = (await this.socket.read(1))[0];
      if (code != RESPONSE_OK) {
        throw `Server response ${code}`;
      }
      let buf = Buffer.from(this.name, "utf8");
      let len_buf = Buffer.alloc(2);
      len_buf.writeInt16LE(buf.length);
      await this.socket.writeAll(len_buf);
      await this.socket.writeAll(buf);
      code = (await this.socket.read(1))[0];
      if (code != RESPONSE_OK) {
        throw `Server response ${code}`;
      }
      this.connected = true;
      process.nextTick(() => this._t_reader(this));
      process.nextTick(() => this._t_ping(this));
    } finally {
      release();
    }
  }
  async _t_reader(me) {
    try {
      let socket = me.socket;
      while (me.connected) {
        let buf = await socket.read(6);
        if (buf[0] == OP_NOP) {
          continue;
        } else if (buf[0] == OP_ACK) {
          let op_id = buf.readInt32LE(1);
          let o = me.frames[op_id];
          delete me.frames[op_id];
          if (o) {
            o.result = buf[5];
            o.release();
          } else {
            console.log(`warning: orphaned elbus frame ack ${op_id}`);
          }
        } else {
          let frame = new Frame();
          frame.type = buf[0];
          let frame_len = buf.readInt32LE(1);
          frame.buf = await socket.read(frame_len);
          if (frame.buf.length != frame_len) {
            console.log(
              `Broken elbus frame: ${frame.buf.length} / ${frame_len}`
            );
          }
          let i = frame.buf.indexOf(0);
          if (i == -1) {
            throw "Invalid elbus frame";
          }
          frame.sender = frame.buf.slice(0, i).toString();
          if (frame.type == OP_PUBLISH) {
            let t = frame.buf.indexOf(0);
            if (t == -1) {
              throw "Invalid elbus frame";
            }
            frame.topic = frame.buf.slice(0, t).toString();
            i += t + 2;
          }
          frame.payload_pos = i + 1;
          if (me.on_frame) {
            process.nextTick(() => me.on_frame(frame));
          }
        }
      }
    } catch (err) {
      await me._handle_daemon_exception(me, err);
    }
  }
  async _handle_daemon_exception(me, e) {
    let release = await me.mgmt_lock.acquire();
    let connected = me.connected;
    try {
      me._disconnect(me);
    } finally {
      release();
      if (connected && me.on_disconnect) {
        console.log(`elbus error: ${e}`);
        process.nextTick(() => me.on_disconnect());
      }
    }
  }
  async send(target, frame) {
    let release = await this.socket_lock.acquire();
    if (frame === undefined) {
      frame = target;
      target = undefined;
    }
    try {
      this.frame_id += 1;
      if (this.frame_id > 0xffff_ffff) {
        this.frame_id = 1;
      }
      let frame_id = this.frame_id;
      let o = new ClientFrame(frame.qos);
      try {
        if (frame.qos > 0) {
          await o.lock_frame();
          this.frames[frame_id] = o;
        }
        let flags = frame.type | (frame.qos << 6);
        if (frame.type == OP_SUBSCRIBE || frame.type == OP_UNSUBSCRIBE) {
          let payload;
          if (Array.isArray(frame.topic)) {
            let p = [];
            frame.topic.map((v) => {
              p.push(Buffer.from(v, "utf8"));
              p.push(Buffer.alloc(1));
            });
            p.pop();
            payload = Buffer.concat(p);
          } else {
            payload = Buffer.from(frame.topic, "utf8");
          }
          let header = Buffer.alloc(9);
          header.writeInt32LE(frame_id);
          header[4] = flags;
          header.writeInt32LE(payload.length, 5);
          await this.socket.write(Buffer.concat([header, payload]));
          return o;
        } else {
          let target_buf = Buffer.from(target, "utf8");
          let frame_len = target_buf.length + frame.payload.length + 1;
          if (frame.header) {
            frame_len += frame.header.length;
          }
          if (frame_len > 0xffff_ffff) {
            throw "frame too large";
          }
          let header = Buffer.alloc(9);
          header.writeInt32LE(frame_id);
          header[4] = flags;
          header.writeInt32LE(frame_len, 5);
          let bufs = [header, target_buf, Buffer.alloc(1)];
          if (frame.header) {
            bufs.push(frame.header);
          }
          await this.socket.write(Buffer.concat(bufs));
          await this.socket.write(frame.payload);
          return o;
        }
      } catch (err) {
        delete this.frames[frame_id];
        throw err;
      }
    } finally {
      release();
    }
  }
  async _t_ping(me) {
    let socket = me.socket;
    try {
      while (me.connected) {
        let release = await me.socket_lock.acquire();
        try {
          await socket.write(PING_FRAME);
        } finally {
          release();
        }
        await sleep(me.ping_interval * 1000);
      }
    } catch (err) {
      await me._handle_daemon_exception(me, err);
    }
  }
  async disconnect() {
    let release = await this.mgmt_lock.acquire();
    let connected = this.connected;
    try {
      this._disconnect(this);
    } finally {
      release();
      if (connected && this.on_disconnect) {
        process.nextTick(() => this.on_disconnect());
      }
    }
  }
  _disconnect(me) {
    me.socket.destroy();
    me.connected = false;
  }
  is_connected() {
    return this.connected;
  }
  async subscribe(topics) {
    let frame = new Frame(OP_SUBSCRIBE, 1);
    frame.topic = topics;
    return await this.send(null, frame);
  }
  async unsubscribe(topics) {
    let frame = new Frame(OP_UNSUBSCRIBE, 1);
    frame.topic = topics;
    return await this.send(null, frame);
  }
}

exports.Client = Client;
exports.Frame = Frame;

exports.Rpc = Rpc;
exports.RpcNotification = RpcNotification;
exports.RpcRequest = RpcRequest;

exports.OP_PUBLISH = OP_PUBLISH;
exports.OP_SUBSCRIBE = OP_SUBSCRIBE;
exports.OP_UNSUBSCRIBE = OP_UNSUBSCRIBE;
exports.OP_MESSAGE = OP_MESSAGE;
exports.OP_BROADCAST = OP_BROADCAST;

exports.RESPONSE_OK = RESPONSE_OK;

exports.ERR_CLIENT_NOT_REGISTERED = ERR_CLIENT_NOT_REGISTERED;
exports.ERR_DATA = ERR_DATA;
exports.ERR_IO = ERR_IO;
exports.ERR_OTHER = ERR_OTHER;
exports.ERR_NOT_SUPPORTED = ERR_NOT_SUPPORTED;
exports.ERR_BUSY = ERR_BUSY;
exports.ERR_NOT_DELIVERED = ERR_NOT_DELIVERED;
exports.ERR_TIMEOUT = ERR_TIMEOUT;
