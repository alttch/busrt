const net = require("net");
const sleep = require("sleep-promise");
const { Mutex, Releaser } = require("async-mutex");
const { PromiseSocket } = require("promise-socket");

const GREETINGS = 0xeb;
const PROTOCOL_VERSION = 1;
const PING_FRAME = Buffer.from([0, 0, 0, 0, 0, 0, 0, 0, 0]);

export const RESPONSE_OK = 1;

export enum BusOp {
  Nop = 0,
  Publish = 1,
  Subscribe = 2,
  Unsubscribe = 3,
  Message = 0x12,
  Broadcast = 0x13,
  Ack = 0xfe
}

export enum QoS {
  No = 0,
  Processed = 1,
  Realtime = 2,
  RealtimeProcessed = 3
}

export enum BusError {
  ClientNotRegistered = 0x71,
  Data = 0x72,
  Io = 0x73,
  Other = 0x74,
  NotSupported = 0x75,
  Busy = 0x76,
  NotDelivered = 0x77,
  Timeout = 0x78
}

export enum RpcOp {
  Notification = 0x00,
  Request = 0x01,
  Reply = 0x11,
  Error = 0x12
}

export enum RpcErrorCode {
  Parse = -32700,
  InvalidRequest = -32600,
  MethodNotFound = -32601,
  InvalidMethodParams = -32602,
  Internal = -32603
}

export interface FrameInterface {
  payload?: Buffer;
  header?: Buffer;
  topic?: Array<string> | string;
  qos: QoS;
  busOp: BusOp;
}

export class Frame implements FrameInterface {
  busOp: BusOp;
  qos: number;
  payload?: Buffer;
  header?: Buffer;
  payloadPos?: number;
  sender?: string;
  primary_sender?: string;
  topic?: Array<string> | string;

  constructor(op: BusOp, qos: number = QoS.Processed) {
    this.busOp = op === undefined ? BusOp.Message : op;
    this.qos = qos === undefined ? QoS.Processed : qos;
  }

  getPayload(): Buffer | undefined {
    return this.payload?.slice(this.payloadPos);
  }
}

class ClientFrame {
  qos: QoS;
  locker: typeof Mutex;
  result?: number;
  release?: Promise<typeof Releaser>;

  constructor(qos: QoS = QoS.No) {
    this.qos = qos;
    if ((qos & 0b1) != 0) {
      this.locker = new Mutex();
    }
  }

  isCompleted(): boolean {
    return (this.qos & 0b1) == 0 ? true : this.result !== undefined;
  }

  async lock(): Promise<void> {
    this.release = await this.locker.acquire();
  }

  async waitCompleted(): Promise<number | undefined> {
    if ((this.qos & 0b1) == 0) {
      return;
    }
    const r = await this.locker.acquire();
    r();
    return this.result;
  }
}

class RpcCallEvent {
  completed: typeof Mutex;
  frame?: Frame;
  error?: RpcError;
  release?: Promise<typeof Releaser>;

  constructor() {
    this.completed = new Mutex();
  }

  isCompleted(): boolean {
    return this.frame !== undefined || this.error !== undefined;
  }

  async lock(): Promise<void> {
    this.release = await this.completed.acquire();
  }

  async waitCompleted(): Promise<RpcCallEvent> {
    const r = await this.completed.acquire();
    r();
    if (this.error) {
      throw this.error;
    } else {
      return this;
    }
  }

  getPayload() {
    return this.frame?.payload?.slice((this.frame?.payloadPos || 0) + 5);
  }
}

class RpcEvent {
  op: RpcOp;
  frame: Frame;
  payloadPos: number;
  method?: Buffer;
  callId?: number;

  constructor(op: RpcOp, frame: Frame, payloadPos: number) {
    this.op = op;
    this.frame = frame;
    this.payloadPos = payloadPos;
  }
  getPayload() {
    return this.frame?.payload?.slice(
      (this.frame?.payloadPos || 0) + this.payloadPos
    );
  }
}

class RpcNotification implements FrameInterface {
  payload?: Buffer;
  qos: QoS;
  header: Buffer;
  busOp: BusOp;

  constructor(payload?: Buffer) {
    this.payload = payload;
    this.qos = QoS.Processed;
    this.header = Buffer.from([RpcOp.Notification]);
    this.busOp = BusOp.Message;
  }
}

class RpcRequest implements FrameInterface {
  payload?: Buffer;
  method: Buffer;
  header?: Buffer;
  qos: QoS;
  busOp: BusOp;

  constructor(method: string, params?: Buffer) {
    this.payload = params;
    this.qos = QoS.Processed;
    this.method = Buffer.from(method, "utf8");
    this.busOp = BusOp.Message;
  }
}

class RpcReply implements FrameInterface {
  payload?: Buffer;
  header?: Buffer;
  qos: QoS;
  busOp: BusOp;

  constructor(result?: Buffer) {
    this.payload = result;
    this.qos = QoS.Processed;
    this.busOp = BusOp.Message;
  }
}

export class RpcError {
  code: number;
  message: any;

  constructor(code: number, message?: any) {
    this.code = code;
    if (message !== undefined) {
      this.message = message;
    } else {
      this.message = Buffer.from(`RPC error code ${code}`);
    }
  }
}

export class Rpc {
  client: Client;
  callId: number;
  call_lock: typeof Mutex;
  calls: Map<number, RpcCallEvent>;
  onFrame?: (frame: RpcEvent) => void;
  onNotification?: (notification: RpcEvent) => void;
  onCall?: (call: RpcEvent) => Promise<Buffer>;

  constructor(client: Client) {
    this.client = client;
    this.client.onFrame = (frame: Frame) => {
      this._handleFrame(frame, this);
    };
    this.callId = 0;
    this.call_lock = new Mutex();
    this.calls = new Map();
    this.onCall = () => {
      throw new RpcError(
        RpcErrorCode.MethodNotFound,
        Buffer.from("RPC engine not intialized")
      );
    };
  }

  isConnected(): boolean {
    return this.client.connected;
  }

  notify(target: string, payload?: Buffer, qos?: QoS): Promise<ClientFrame> {
    const notification = new RpcNotification(payload);
    if (qos !== undefined) notification.qos = qos;
    return this.client.send(target, notification);
  }

  call0(
    target: string,
    method: string,
    params?: Buffer,
    qos?: QoS
  ): Promise<ClientFrame> {
    const request = new RpcRequest(method, params);
    if (qos !== undefined) request.qos = qos;
    request.header = Buffer.concat([
      Buffer.from([RpcOp.Request, 0, 0, 0, 0]),
      request.method,
      Buffer.alloc(1)
    ]);
    return this.client.send(target, request);
  }

  async call(
    target: string,
    method: string,
    params?: Buffer,
    qos?: QoS
  ): Promise<RpcCallEvent> {
    const release = await this.call_lock.acquire();
    const callId = this.callId + 1;
    this.callId = callId == 0xffff_ffff ? 0 : callId;
    release();
    const callEvent = new RpcCallEvent();
    await callEvent.lock();
    this.calls.set(callId, callEvent);
    const callIdBuf = Buffer.alloc(4);
    callIdBuf.writeUInt32LE(callId);
    const request = new RpcRequest(method, params);
    if (qos !== undefined) request.qos = qos;
    request.header = Buffer.concat([
      Buffer.from([RpcOp.Request]),
      callIdBuf,
      request.method,
      Buffer.alloc(1)
    ]);
    try {
      const opc = await this.client.send(target, request);
      const code = await opc.waitCompleted();
      if (code != RESPONSE_OK) {
        this.calls.delete(callId);
        const err_code = -32000 - (code || 0);
        callEvent.error = new RpcError(err_code);
        (callEvent as any).release();
      }
    } catch (err: any) {
      this.calls.delete(callId);
      const err_code = -32000 - BusError.Io;
      callEvent.error = new RpcError(err_code, Buffer.from(err.toString()));
      (callEvent as any).release();
    }
    return callEvent;
  }

  async _handleFrame(frame: Frame, me: Rpc) {
    if (frame.busOp == BusOp.Message) {
      const opCode = (frame.payload as Buffer)[frame.payloadPos || 0];
      if (opCode == RpcOp.Notification) {
        if (me.onNotification) {
          const ev = new RpcEvent(opCode, frame, 1);
          await me.onNotification(ev);
        }
      } else if (opCode == RpcOp.Request) {
        const sender = frame.sender;
        const callIdBuf = (frame.payload as Buffer).slice(
          (frame.payloadPos || 0) + 1,
          (frame.payloadPos || 0) + 5
        );
        const callId = callIdBuf.readUInt32LE();
        const s = (frame.payload as Buffer).slice((frame.payloadPos || 0) + 5);
        let i = s.indexOf(0);
        if (i == -1) {
          throw "Invalid BUS/RT RPC frame";
        }
        const method = s.slice(0, i);
        const ev = new RpcEvent(RpcOp.Request, frame, 6 + method.length);
        ev.callId = callId;
        ev.method = method;
        if (callId == 0) {
          if (me.onCall) {
            await me.onCall(ev);
          }
        } else {
          const reply = new RpcReply();
          try {
            if (me.onCall) {
              reply.payload = await me.onCall(ev);
              if (reply.payload === null || reply.payload === undefined) {
                reply.payload = Buffer.alloc(0);
              }
              reply.header = Buffer.concat([
                Buffer.from([RpcOp.Reply]),
                callIdBuf
              ]);
            }
          } catch (err: any) {
            const code =
              err.code === undefined ? RpcErrorCode.Internal : err.code;
            const codeBuf = Buffer.alloc(2);
            codeBuf.writeInt16LE(code);
            reply.header = Buffer.concat([
              Buffer.from([RpcOp.Error]),
              callIdBuf,
              codeBuf
            ]);
            reply.payload = err.message;
            if (reply.payload === undefined) {
              reply.payload = Buffer.from(err.toString());
            }
          }
          await me.client.send(sender || "", reply);
        }
      } else if (opCode == RpcOp.Reply || opCode == RpcOp.Error) {
        const callId = (frame.payload as Buffer).readUInt32LE(
          (frame.payloadPos || 0) + 1
        );
        const callEvent = me.calls.get(callId);
        if (callEvent) {
          me.calls.delete(callId);
          callEvent.frame = frame;
          if (opCode == RpcOp.Error) {
            const err_code = (frame.payload as Buffer).readInt16LE(
              (frame.payloadPos || 0) + 5
            );
            callEvent.error = new RpcError(
              err_code,
              (frame.payload as Buffer).slice((frame.payloadPos || 0) + 7)
            );
          }
          (callEvent as any).release();
        } else {
          console.warn(`orphaned RPC response: ${callId}`);
        }
      } else {
        throw `Invalid RPC frame code ${opCode}`;
      }
    } else if (me.onFrame) {
      process.nextTick(() => (me as any).onFrame(frame));
    }
  }
}

export class Client {
  name: string;
  onFrame?: (frame: Frame) => void;
  onDisconnect?: () => void;
  ping_interval: number;
  connected: boolean;
  timeout: number;
  mgmt_lock: typeof Mutex;
  socket_lock: typeof Mutex;
  frameId: number;
  frames: Map<number, ClientFrame>;
  socket: typeof PromiseSocket;

  constructor(name: string) {
    if (name === undefined) {
      throw "name is not defined";
    }
    this.name = name;
    this.ping_interval = 1;
    this.timeout = 5;
    this.connected = false;
    this.mgmt_lock = new Mutex();
    this.socket_lock = new Mutex();
    this.frameId = 0;
    this.frames = new Map();
  }

  async connect(path: string): Promise<void> {
    const release = await this.mgmt_lock.acquire();
    try {
      const sock = new net.Socket();
      sock.setNoDelay(true);
      this.socket = new PromiseSocket(sock);
      this.socket.setTimeout(this.timeout * 1000);
      await this.socket.connect(path);
      const header = await this.socket.read(3);
      if (header[0] != GREETINGS) {
        throw "Unsupported protocol";
      }
      const ver = Buffer.from(header.slice(1, 3));
      if (ver.readUInt16LE(0) != PROTOCOL_VERSION) {
        throw "Unsupported protocol version";
      }
      await this.socket.writeAll(header);
      let code = (await this.socket.read(1))[0];
      if (code != RESPONSE_OK) {
        throw `Server response ${code}`;
      }
      let buf = Buffer.from(this.name, "utf8");
      let len_buf = Buffer.alloc(2);
      len_buf.writeUInt16LE(buf.length);
      await this.socket.writeAll(len_buf);
      await this.socket.writeAll(buf);
      code = (await this.socket.read(1))[0];
      if (code != RESPONSE_OK) {
        throw `Server response ${code}`;
      }
      this.connected = true;
      process.nextTick(() => this._tReader(this));
      process.nextTick(() => this._tPing(this));
    } finally {
      release();
    }
  }

  async _tReader(me: Client): Promise<void> {
    try {
      let socket = me.socket;
      while (me.connected) {
        const buf = await socket.read(6);
        if (buf[0] == BusOp.Nop) {
          continue;
        } else if (buf[0] == BusOp.Ack) {
          const opId = buf.readUInt32LE(1);
          const o = me.frames.get(opId);
          me.frames.delete(opId);
          if (o) {
            o.result = buf[5];
            (o as any).release();
          } else {
            console.warn(`orphaned BUS/RT frame ack ${opId}`);
          }
        } else {
          let frame = new Frame(buf[0]);
          let frame_len = buf.readUInt32LE(1);
          frame.payload = await socket.read(frame_len);
          if (frame.payload?.length != frame_len) {
            console.warn(
              `broken BUS/RT frame: ${frame.payload?.length} / ${frame_len}`
            );
          }
          let i = (frame.payload as Buffer).indexOf(0);
          if (i == -1) {
            throw "Invalid BUS/RT frame";
          }
          frame.sender = (frame.payload as Buffer).slice(0, i).toString();
          frame.primary_sender = frame.sender.split("%%", 1)[0];
          i += 1; // Move the cursor past the sender's null terminator

          if (frame.busOp == BusOp.Publish) {
            let t = (frame.payload as Buffer).slice(i).indexOf(0);
            if (t == -1) {
              throw "Invalid BUS/RT frame";
            }
            frame.topic = (frame.payload as Buffer).slice(i, i + t).toString();
            i += t + 1; // Move the cursor past the topic's null terminator
          }
          frame.payloadPos = i;
          if (me.onFrame) {
            process.nextTick(() => (me.onFrame as any)(frame));
          }
        }
      }
    } catch (err) {
      await me._handleDaemonException(me, err);
    }
  }

  async _handleDaemonException(me: Client, e: any) {
    let release = await me.mgmt_lock.acquire();
    let connected = me.connected;
    try {
      me._disconnect(me);
    } finally {
      release();
      if (connected) {
        console.error(`BUS/RT error: ${e}`);
        if (me.onDisconnect) {
          process.nextTick(() => (me.onDisconnect as any)());
        }
      }
    }
  }

  send0(frame: FrameInterface) {
    return this._send(frame);
  }

  send(target: string, frame: FrameInterface) {
    return this._send(frame, target);
  }

  async _send(frame: FrameInterface, target?: string) {
    const release = await this.socket_lock.acquire();
    try {
      this.frameId += 1;
      if (this.frameId > 0xffff_ffff) {
        this.frameId = 1;
      }
      const frameId = this.frameId;
      const o = new ClientFrame(frame.qos);
      try {
        if ((frame.qos & 0b1) != 0) {
          await o.lock();
          this.frames.set(frameId, o);
        }
        let flags = frame.busOp | (frame.qos << 6);
        if (
          frame.busOp == BusOp.Subscribe ||
          frame.busOp == BusOp.Unsubscribe
        ) {
          let payload;
          if (Array.isArray(frame.topic)) {
            const p: Array<Buffer> = [];
            frame.topic.map((v) => {
              p.push(Buffer.from(v, "utf8"));
              p.push(Buffer.alloc(1));
            });
            p.pop();
            payload = Buffer.concat(p);
          } else {
            payload = Buffer.from(frame.topic || "", "utf8");
          }
          const header = Buffer.alloc(9);
          header.writeUInt32LE(frameId);
          header[4] = flags;
          header.writeUInt32LE(payload.length, 5);
          await this.socket.write(Buffer.concat([header, payload]));
          return o;
        } else {
          const target_buf = Buffer.from(target || "", "utf8");
          let frame_len = target_buf.length + (frame.payload?.length || 0) + 1;
          if (frame.header) {
            frame_len += frame.header.length;
          }
          if (frame_len > 0xffff_ffff) {
            throw "frame too large";
          }
          const header = Buffer.alloc(9);
          header.writeUInt32LE(frameId);
          header[4] = flags;
          header.writeUInt32LE(frame_len, 5);
          const bufs = [header, target_buf, Buffer.alloc(1)];
          if (frame.header) {
            bufs.push(frame.header);
          }
          await this.socket.write(Buffer.concat(bufs));
          if (frame.payload !== undefined) {
            await this.socket.write(frame.payload);
          }
          return o;
        }
      } catch (err) {
        this.frames.delete(frameId);
        throw err;
      }
    } finally {
      release();
    }
  }

  async _tPing(me: Client) {
    const socket = me.socket;
    try {
      while (me.connected) {
        const release = await me.socket_lock.acquire();
        try {
          await socket.write(PING_FRAME);
        } finally {
          release();
        }
        await sleep(me.ping_interval * 1000);
      }
    } catch (err) {
      await me._handleDaemonException(me, err);
    }
  }

  async disconnect() {
    let release = await this.mgmt_lock.acquire();
    let connected = this.connected;
    try {
      this._disconnect(this);
    } finally {
      release();
      if (connected && this.onDisconnect) {
        process.nextTick(() => (this.onDisconnect as any)());
      }
    }
  }

  _disconnect(me: Client) {
    me.socket.destroy();
    me.connected = false;
  }

  isConnected() {
    return this.connected;
  }

  async subscribe(topics: Array<string> | string) {
    const frame = new Frame(BusOp.Subscribe, QoS.Processed);
    frame.topic = topics;
    return await this._send(frame);
  }

  async unsubscribe(topics: Array<string>) {
    const frame = new Frame(BusOp.Unsubscribe, QoS.Processed);
    frame.topic = topics;
    return await this._send(frame);
  }

  async publish(topic: string, payload?: Buffer, qos?: QoS) {
    const frame = new Frame(BusOp.Publish, qos);
    frame.payload = payload;
    return await this._send(frame, topic);
  }
}
