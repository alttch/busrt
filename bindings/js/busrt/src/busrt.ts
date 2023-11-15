import { Socket } from "node:net";
import sleep from "sleep-promise";
import { Mutex } from "async-mutex";
import { PromiseSocket } from "promise-socket";

const GREETINGS = 0xeb;
const PROTOCOL_VERSION = 1;
const PING_FRAME = Buffer.from([0, 0, 0, 0, 0, 0, 0, 0, 0]);
const RESPONSE_OK = 1;

enum BusOp {
  Nop = 0,
  Publish = 1,
  Subscribe = 2,
  Unsubscribe = 3,
  Message = 0x12,
  Broadcast = 0x13,
  Ack = 0xfe
}

/** Bus call QoS */
export enum QoS {
  No = 0,
  Processed = 1,
  Realtime = 2,
  RealtimeProcessed = 3
}

/** Bus call errors (-32000 - code) */
export enum BusErrorCode {
  BusNotRegistered = -32113,
  Data = -32114,
  Io = -32115,
  Other = -32116,
  NotSupported = -32117,
  Busy = -32118,
  NotDelivered = -32119,
  Timeout = -32120,
  Access = -32121,
  RpcParse = -32700,
  RpcInvalidRequest = -32600,
  RpcMethodNotFound = -32601,
  RpcInvalidMethodParams = -32602,
  RpcInternal = -32603
}

enum RpcOp {
  Notification = 0x00,
  Request = 0x01,
  Reply = 0x11,
  Error = 0x12
}

/** @ignore */
interface FrameInterface {
  payload?: Buffer;
  header?: Buffer;
  op_topic?: Array<string> | string;
  qos: QoS;
  busOp: BusOp;
}

/** Bus frame */
export class Frame implements FrameInterface {
  /** @ignore */
  busOp: BusOp;
  /** @ignore */
  qos: number;
  /** @ignore */
  payload?: Buffer;
  /** @ignore */
  header?: Buffer;
  /** @ignore */
  payloadPos?: number;
  /** use this field to get the real frame sender */
  primary_sender?: string;
  /** frame sender with an optional secondary suffix */
  sender?: string;
  /** frame topic (in incoming publish events) */
  topic?: string;
  /** @ignore */
  op_topic?: string | Array<string>;

  /** @ignore */
  constructor(op: BusOp, qos: number = QoS.Processed) {
    this.busOp = op === undefined ? BusOp.Message : op;
    this.qos = qos === undefined ? QoS.Processed : qos;
  }

  /**
   * Gets frame payload
   *
   * @returns {Buffer}
   */
  getPayload(): Buffer {
    // payload is always set for incoming frames
    return (this.payload as Buffer).slice(this.payloadPos);
  }
}

/** Bus operation result */
export class OpResult {
  /** @ignore */
  qos: QoS;
  /** @ignore */
  locker?: Mutex;
  /** @ignore */
  result?: number;
  /** @ignore */
  release?: Promise<any>;

  /** @ignore */
  constructor(qos: QoS = QoS.No) {
    this.qos = qos;
    if ((qos & 0b1) != 0) {
      this.locker = new Mutex();
    }
  }

  /**
   * Is operation completed
   *
   * @returns {boolean}
   */
  isCompleted(): boolean {
    return (this.qos & 0b1) == 0 ? true : this.result !== undefined;
  }

  /** @ignore */
  async lock(): Promise<void> {
    this.release = (await this.locker?.acquire()) as any;
  }

  /** @ignore */
  async _waitCompletedCode(): Promise<number | undefined> {
    if ((this.qos & 0b1) == 0) {
      return;
    }
    const r = await this.locker?.acquire();
    if (r) {
      r();
    }
    return this.result;
  }

  /**
   * Waits until the operation is completed
   *
   * @returns {Promise<void>}
   * @throws {BusError}
   */
  async waitCompleted(): Promise<void> {
    const code = await this._waitCompletedCode();
    if (code !== undefined && code !== RESPONSE_OK) {
      throw new BusError(-32000 - code);
    }
  }
}

export class RpcOpResult {
  /** @ignore */
  completed: Mutex;
  /** @ignore */
  frame?: Frame;
  /** @ignore */
  error?: BusError;
  /** @ignore */
  release?: Promise<any>;

  /** @ignore */
  constructor() {
    this.completed = new Mutex();
  }

  /**
   * Is operation completed
   *
   * @returns {boolean}
   */
  isCompleted(): boolean {
    return this.frame !== undefined || this.error !== undefined;
  }

  /** @ignore */
  async lock(): Promise<void> {
    this.release = (await this.completed.acquire()) as any;
  }

  /**
   * Waits until the operation is completed
   *
   * @returns {Promise<RpcOpResult>}
   * @throws {BusError}
   */
  async waitCompleted(): Promise<RpcOpResult> {
    const r = await this.completed.acquire();
    r();
    if (this.error) {
      throw this.error;
    } else {
      return this;
    }
  }

  /**
   * Gets frame payload
   *
   * @returns {Buffer}
   */
  getPayload(): Buffer {
    // payload is always set for incoming frames
    return ((this.frame as Frame).payload as Buffer).slice(
      (this.frame?.payloadPos || 0) + 5
    );
  }
}

/**
 * Incoming RPC-layer events
 */
export class RpcEvent {
  /** @ignore */
  op: RpcOp;
  /** bus frame */
  frame: Frame;
  /** @ignore */
  payloadPos: number;
  /* RPC method (in RPC calls) */
  method?: Buffer;
  /** @ignore */
  callId?: number;

  /** @ignore */
  constructor(op: RpcOp, frame: Frame, payloadPos: number) {
    this.op = op;
    this.frame = frame;
    this.payloadPos = payloadPos;
  }

  /**
   * Gets frame payload
   *
   * @returns {Buffer}
   */
  getPayload(): Buffer | undefined {
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

/** Bus errors */
export class BusError {
  /** Error code */
  code: number;
  /** Error message */
  message?: Buffer;

  /**
   * @param {number} code - error code
   * @param {Buffer} [message] - encoded message payload
   */
  constructor(code: number, message?: Buffer) {
    this.code = code;
    this.message = message;
  }
}

/** Bus RPC layer */
export class Rpc {
  /** attached bus client */
  client: Bus;
  /** @ignore */
  callId: number;
  /** @ignore */
  call_lock: Mutex;
  /** @ignore */
  calls: Map<number, RpcOpResult>;
  /** Method, called on incoming frames */
  onFrame?: (frame: RpcEvent) => Promise<void> | void;
  /** Method, called on incoming RPC notifications */
  onNotification?: (notification: RpcEvent) => Promise<void> | void;
  /** Method, called on incoming RPC calls */
  onCall: (call: RpcEvent) => Promise<Buffer | undefined> | Buffer | undefined;
  blockingNotifications: boolean;
  blockingFrames: boolean;

  /**
   * @param {Bus} client - Bus client
   */
  constructor(client: Bus) {
    this.client = client;
    this.client.onFrame = (frame: Frame) => {
      this._handleFrame(frame, this);
    };
    this.callId = 0;
    this.call_lock = new Mutex();
    this.calls = new Map();
    this.onCall = () => {
      throw new BusError(
        BusErrorCode.RpcMethodNotFound,
        Buffer.from("RPC engine not intialized")
      );
    };
    this.blockingNotifications = false;
    this.blockingFrames = false;
  }

  /**
   * Is RPC client connected
   *
   * @returns {boolean}
   */
  isConnected(): boolean {
    return this.client.connected;
  }

  /**
   * Sends RPC notification
   *
   * @param {string} target - notification target
   * @param {Buffer} [payload] - payload
   * @param {QoS} [qos] - QoS
   *
   * @returns {Promise<OpResult>}
   */
  notify(target: string, payload?: Buffer, qos?: QoS): Promise<OpResult> {
    const notification = new RpcNotification(payload);
    if (qos !== undefined) notification.qos = qos;
    return this.client._send(notification, target);
  }

  /**
   * Performs RPC call with no reply required
   *
   * @param {string} target - RPC target
   * @param {string} method - RPC method on the target
   * @param {Buffer} [payload] - payload
   * @param {QoS} [qos] - QoS
   *
   * @returns {Promise<OpResult>}
   */
  call0(
    target: string,
    method: string,
    params?: Buffer,
    qos?: QoS
  ): Promise<OpResult> {
    const request = new RpcRequest(method, params);
    if (qos !== undefined) request.qos = qos;
    request.header = Buffer.concat([
      Buffer.from([RpcOp.Request, 0, 0, 0, 0]),
      request.method,
      Buffer.alloc(1)
    ]);
    return this.client._send(request, target);
  }

  /**
   * Performs RPC call
   *
   * @param {string} target - RPC target
   * @param {string} method - RPC method on the target
   * @param {Buffer} [payload] - payload
   * @param {QoS} [qos] - QoS
   *
   * @returns {Promise<RpcOpResult>}
   */
  async call(
    target: string,
    method: string,
    params?: Buffer,
    qos?: QoS
  ): Promise<RpcOpResult> {
    const release = await this.call_lock.acquire();
    const callId = this.callId + 1;
    this.callId = callId == 0xffff_ffff ? 0 : callId;
    release();
    const rpcOpResult = new RpcOpResult();
    await rpcOpResult.lock();
    this.calls.set(callId, rpcOpResult);
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
      const opc = await this.client._send(request, target);
      const code = await opc._waitCompletedCode();
      if (code !== undefined && code != RESPONSE_OK) {
        this.calls.delete(callId);
        rpcOpResult.error = new BusError(-32000 - code);
        (rpcOpResult as any).release();
      }
    } catch (err: any) {
      this.calls.delete(callId);
      const err_code = BusErrorCode.Io;
      rpcOpResult.error = new BusError(err_code, Buffer.from(err.toString()));
      (rpcOpResult as any).release();
    }
    return rpcOpResult;
  }

  /** @ignore */
  async _handleFrame(frame: Frame, me: Rpc): Promise<void> {
    if (frame.busOp == BusOp.Message) {
      const opCode = (frame.payload as Buffer)[frame.payloadPos || 0];
      if (opCode == RpcOp.Notification) {
        if (me.onNotification) {
          const ev = new RpcEvent(opCode, frame, 1);
          if (me.blockingNotifications) {
            await me.onNotification(ev);
          } else {
            process.nextTick(() => (me as any).onNotification(ev));
          }
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
          reply.qos = frame.qos;
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
              err.code === undefined ? BusErrorCode.RpcInternal : err.code;
            const codeBuf = Buffer.alloc(2);
            codeBuf.writeInt16LE(code);
            reply.header = Buffer.concat([
              Buffer.from([RpcOp.Error]),
              callIdBuf,
              codeBuf
            ]);
            reply.payload =
              err.message === undefined ? Buffer.alloc(0) : err.message;
          }
          if (sender) {
            await me.client._send(reply, sender);
          }
        }
      } else if (opCode == RpcOp.Reply || opCode == RpcOp.Error) {
        const callId = (frame.payload as Buffer).readUInt32LE(
          (frame.payloadPos || 0) + 1
        );
        const rpcOpResult = me.calls.get(callId);
        if (rpcOpResult) {
          me.calls.delete(callId);
          rpcOpResult.frame = frame;
          if (opCode == RpcOp.Error) {
            const err_code = (frame.payload as Buffer).readInt16LE(
              (frame.payloadPos || 0) + 5
            );
            rpcOpResult.error = new BusError(
              err_code,
              (frame.payload as Buffer).slice((frame.payloadPos || 0) + 7)
            );
          }
          (rpcOpResult as any).release();
        } else {
          console.warn(`orphaned RPC response: ${callId}`);
        }
      } else {
        throw `Invalid RPC frame code ${opCode}`;
      }
    } else if (me.onFrame) {
      const ev = new RpcEvent(RpcOp.Request, frame, 1);
      if (me.blockingFrames) {
        await me.onFrame(ev);
      } else {
        process.nextTick(() => (me as any).onFrame(ev));
      }
    }
  }
}

/** BUS/RT client */
export class Bus {
  /** assigned name */
  name: string;
  /** called on incoming frames */
  onFrame?: (frame: Frame) => Promise<void> | void;
  /** called on disconnect */
  onDisconnect?: () => void;
  /** broker ping interval */
  pingInterval: number;
  /** @ignore */
  connected: boolean;
  /** bus timeout */
  timeout: number;
  /** @ignore */
  mgmt_lock: Mutex;
  /** @ignore */
  socket_lock: Mutex;
  /** @ignore */
  frameId: number;
  /** @ignore */
  frames: Map<number, OpResult>;
  /** @ignore */
  socket: PromiseSocket<any>;

  /**
   * @param {string} name - client name
   */
  constructor(name: string) {
    if (name === undefined) {
      throw "name is not defined";
    }
    this.name = name;
    this.pingInterval = 1;
    this.timeout = 5;
    this.connected = false;
    this.mgmt_lock = new Mutex();
    this.socket_lock = new Mutex();
    this.frameId = 0;
    this.frames = new Map();
    this.socket = undefined as any;
  }

  /**
   * Connects the client
   *
   * @param {string} path - UNIX socket or TCP host:port
   *
   * @returns {Promise<void>}
   */
  async connect(path: string): Promise<void> {
    const release = await this.mgmt_lock.acquire();
    try {
      const sock = new Socket();
      sock.setNoDelay(true);
      this.socket = new PromiseSocket(sock);
      //this.socket.setTimeout(this.timeout * 1000);
      await this.socket.connect(path);
      const header = await this.socket.read(3);
      if (header === undefined) {
        throw "I/O error";
      }
      if (header[0] != GREETINGS) {
        throw "Unsupported protocol";
      }
      const ver = Buffer.from(header.slice(1, 3));
      if (ver.readUInt16LE(0) != PROTOCOL_VERSION) {
        throw "Unsupported protocol version";
      }
      await this.socket.writeAll(header);
      let code = ((await this.socket.read(1)) as Buffer)[0];
      if (code != RESPONSE_OK) {
        throw `Server response ${code}`;
      }
      let buf = Buffer.from(this.name, "utf8");
      let len_buf = Buffer.alloc(2);
      len_buf.writeUInt16LE(buf.length);
      await this.socket.writeAll(len_buf);
      await this.socket.writeAll(buf);
      code = ((await this.socket.read(1)) as Buffer)[0];
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

  /** @ignore */
  async _tReader(me: Bus): Promise<void> {
    try {
      let socket = me.socket;
      while (me.connected) {
        const buf = (await socket.read(6)) as Buffer;
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
          frame.payload = (await socket.read(frame_len)) as Buffer;
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

  /** @ignore */
  async _handleDaemonException(me: Bus, e: any): Promise<void> {
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

  /**
   * Sends a message to the target
   *
   * @param {string} target - bus target
   * @param {Buffer} [payload] - payload
   * @param {QoS} [qos] - QoS
   */
  send(target: string, payload?: Buffer, qos?: QoS): Promise<OpResult> {
    const frame = new Frame(BusOp.Message, qos);
    frame.payload = payload;
    return this._send(frame, target);
  }

  /** @ignore */
  async _send(frame: FrameInterface, target?: string): Promise<OpResult> {
    const release = await this.socket_lock.acquire();
    try {
      this.frameId += 1;
      if (this.frameId > 0xffff_ffff) {
        this.frameId = 1;
      }
      const frameId = this.frameId;
      const o = new OpResult(frame.qos);
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
          if (Array.isArray(frame.op_topic)) {
            const p: Array<Buffer> = [];
            frame.op_topic.map((v) => {
              p.push(Buffer.from(v, "utf8"));
              p.push(Buffer.alloc(1));
            });
            p.pop();
            payload = Buffer.concat(p);
          } else {
            payload = Buffer.from(frame.op_topic || "", "utf8");
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

  /** @ignore */
  async _tPing(me: Bus): Promise<void> {
    const socket = me.socket;
    try {
      while (me.connected) {
        const release = await me.socket_lock.acquire();
        try {
          await socket.write(PING_FRAME);
        } finally {
          release();
        }
        await sleep(me.pingInterval * 1000);
      }
    } catch (err) {
      await me._handleDaemonException(me, err);
    }
  }

  /**
   * Disconnects the bus client
   *
   * @returns {Promise<void>}
   */
  async disconnect(): Promise<void> {
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

  /** @ignore */
  _disconnect(me: Bus): void {
    me.socket.destroy();
    me.connected = false;
  }

  /**
   * Is bus client connected
   *
   * @returns {boolean}
   */
  isConnected(): boolean {
    return this.connected;
  }

  /**
   * Subscribes client to topic(s)
   *
   * @param {string|Array<string>} topics - topics to subscribe
   *
   * @returns {Promise<OpResult>}
   */
  async subscribe(topics: Array<string> | string): Promise<OpResult> {
    const frame = new Frame(BusOp.Subscribe, QoS.Processed);
    frame.op_topic = topics;
    return await this._send(frame);
  }

  /**
   * Unsubscribes client from topic(s)
   *
   * @param {string|Array<string>} topics - topics to unsubscribe
   *
   * @returns {Promise<OpResult>}
   */
  async unsubscribe(topics: Array<string>): Promise<OpResult> {
    const frame = new Frame(BusOp.Unsubscribe, QoS.Processed);
    frame.op_topic = topics;
    return await this._send(frame);
  }

  /**
   * Publishes a message to a topic
   *
   * @param {string} topic - the target topic
   * @param {Buffer} [payload] - payload
   * @param {QoS} [qos] - QoS
   *
   * @returns {Promise<OpResult>}
   */
  async publish(topic: string, payload?: Buffer, qos?: QoS): Promise<OpResult> {
    const frame = new Frame(BusOp.Publish, qos);
    frame.payload = payload;
    return await this._send(frame, topic);
  }
}
