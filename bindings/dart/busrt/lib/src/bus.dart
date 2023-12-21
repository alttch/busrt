import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:busrt/src/consts.dart';
import 'package:busrt/src/bus_error.dart';
import 'package:busrt/src/frame.dart';
import 'package:busrt/src/frame_kind.dart';
import 'package:busrt/src/future_soket.dart';
import 'package:busrt/src/op_result.dart';
import 'package:busrt/src/qos.dart';
import 'package:busrt/src/rpc_event.dart';
import 'package:busrt/src/rpc_event_kind.dart';
import 'package:busrt/src/rpc_op_result.dart';
import 'package:mutex/mutex.dart';

part 'rpc.dart';

class Bus {
  final String _name;
  final Duration _pingInterval;
  final Duration _timeout;
  bool _connected = false;
  final _connLoc = Mutex();
  final _sendLoc = Mutex();
  int _frameId = 0;

  final _utf8decoder = Utf8Decoder();
  final _utf8encoder = Utf8Encoder();

  final _frames = <int, OpResult>{};
  final _soket = FutureSoket();
  FutureOr<void> Function(Frame frame)? _onFrame;
  void Function()? _onDisconnect;

  Bus(
    this._name, {
    Duration pingInterval = defaultPingInterval,
    Duration timeout = defaultTimeout,
  })  : _pingInterval = pingInterval,
        _timeout = timeout;

  Future<void> connect(String path) async {
    await _connLoc.acquire();
    try {
      final (host, port) = _hostAndPortFromPath(path);
      await _soket.connect(host, port, _timeout);
      await _connectionHandler();

      Future.microtask(() => Timer.periodic(_pingInterval, _ping));
      Future.microtask(_reader);
    } finally {
      _connLoc.release();
    }
  }

  Future<OpResult> send(
    String target, [
    Uint8List? payload,
    QoS qos = QoS.processed,
  ]) async {
    final frameKind = target.contains('*') || target.contains('?')
        ? FrameKind.broadcast
        : FrameKind.message;

    final frame = Frame(
      kind: frameKind,
      qos: qos,
      buf: payload ?? Uint8List(0),
    );

    return await _send(frame, [target]);
  }

  Future<OpResult> subscribe(List<String> topics,
      [QoS qos = QoS.processed]) async {
    final frame = Frame(kind: FrameKind.subscribeTopic, qos: qos);

    return await _send(frame, topics);
  }

  Future<OpResult> unsubscribe(List<String> topics,
      [QoS qos = QoS.processed]) async {
    final frame = Frame(kind: FrameKind.unsubscribeTopic, qos: qos);

    return await _send(frame, topics);
  }

  Future<OpResult> publish(String topic,
      [Uint8List? payload, QoS qos = QoS.processed]) async {
    final frame = Frame(kind: FrameKind.publish, qos: qos, buf: payload);
    return await _send(frame, [topic]);
  }

  bool isConnected() => _connected && _soket.isConnected();

  set onFrame(FutureOr<void> Function(Frame frame)? fn) {
    _onFrame = fn;
  }

  set onDisconnect(void Function()? fn) {
    _onDisconnect = fn;
  }

  Future<void> disconnect() async {
    await _connLoc.acquire();
    final connected = isConnected();
    try {
      await _disconnect();
    } finally {
      _connLoc.release();
      if (connected && _onDisconnect is Function) {
        _onDisconnect!();
      }
    }
  }

  Future<OpResult> _send(Frame frame, [List<String>? targets]) async {
    await _sendLoc.acquire();

    late final int frameId;
    try {
      frameId = _incrementFrameId();
      final o = OpResult(frame.qos);
      if (frame.qos.needsAck()) {
        await o.loc();
        _frames[frameId] = o;
      }

      final flags = frame.kind.value | (frame.qos.value << 6);

      switch (frame.kind) {
        case FrameKind.unsubscribeTopic || FrameKind.subscribeTopic:
          final payload =
              _utf8encoder.convert(targets?.join(String.fromCharCode(0)) ?? "");
          final header = _sendHeader(frameId, flags, payload.length);
          _soket.write(Uint8List.fromList([...header, ...payload]));

          return o;

        default:
          final targetBuf = _utf8encoder.convert(targets?.first ?? "");
          final frameLen = targetBuf.length +
              frame.payload.length +
              1 +
              (frame.header?.length ?? 0);

          if (frameLen > 0xffffffff) {
            throw DataError("frame too large");
          }

          final header = _sendHeader(frameId, flags, frameLen);
          final bufs = [...header, ...targetBuf, 0];
          if (frame.header != null) {
            bufs.addAll(frame.header!);
          }

          _soket.write(Uint8List.fromList(bufs));

          if (frame.payload.isNotEmpty) {
            _soket.write(frame.payload);
          }

          return o;
      }
    } catch (e) {
      _frames.remove(frameId);
      rethrow;
    } finally {
      _sendLoc.release();
    }
  }

  Uint8List _sendHeader(int frameId, int flags, int payloadLen) {
    final header = Uint8List.fromList([
      ...Uint32List.fromList([frameId]).buffer.asUint8List(),
      flags,
      ...Uint32List.fromList([payloadLen]).buffer.asUint8List(),
    ]);

    return header;
  }

  int _incrementFrameId() {
    if (_frameId >= 0xffffffff) {
      _frameId = 1;
    } else {
      _frameId += 1;
    }

    return _frameId;
  }

  (InternetAddress, int) _hostAndPortFromPath(String path) {
    final conditional = path.endsWith('.sock') ||
        path.endsWith('.socket') ||
        path.endsWith('.ipc') ||
        path.startsWith('/');

    if (conditional) {
      final host = InternetAddress(path, type: InternetAddressType.unix);
      final port = 0;
      return (host, port);
    }

    final [addr, portStr] = path.split(":");
    final host = InternetAddress(addr, type: InternetAddressType.IPv4);
    final port = int.parse(portStr);
    return (host, port);
  }

  Future<void> _connectionHandler() async {
    if (_name.length > 0xffff) {
      throw DataError("name too long, max length: 0xffff");
    }

    var buf = await _soket.read(3);
    if (buf[0] != greetings) {
      throw NotSupportedError("Invalid greetings");
    }

    if (buf[1] != protocolVersion) {
      throw NotSupportedError("Unsupported protocol version");
    }

    _soket.write(buf);

    buf = await _soket.read(1);

    if (buf[0] != responseOk) {
      throw buf[0].toErrKind("Server greetings response: {$buf[0]}")!;
    }

    final nameLen =
        Uint32List.fromList([_name.length]).buffer.asUint8List().take(2);
    final name = _utf8encoder.convert(_name);

    _soket.write(Uint8List.fromList([...nameLen, ...name]));

    buf = await _soket.read(1);

    if (buf[0] != responseOk) {
      throw buf[0].toErrKind("Server greetings response: {$buf[0]}")!;
    }
    _connected = true;
    return;
  }

  Future<void> _ping(Timer t) async {
    await _sendLoc.acquire();
    try {
      _soket.write(pingFrame);
    } catch (e, s) {
      t.cancel();
      await _handleDaemonException(e, s);
    } finally {
      _sendLoc.release();
    }
  }

  Future<void> _reader() async {
    while (isConnected()) {
      try {
        final buf = await _soket.read(6);
        switch (buf[0].toFrameKind()) {
          case FrameKind.nop:
            continue;
          case FrameKind.acknowledge:
            _askHandler(buf);
            continue;
          default:
            await _incomingMessageHandler(buf);
            continue;
        }
      } catch (e, s) {
        await _handleDaemonException(e, s);
      }
    }
  }

  void _askHandler(Uint8List buf) {
    final opIdBbuf = Uint8List.fromList(buf.getRange(1, 5).toList());
    final opId = opIdBbuf.buffer.asUint32List().first;
    final res = _frames.remove(opId);
    if (res is OpResult) {
      res.setResult(buf[5]);
      res.unloc();
    } else {
      print("orphaned BUS/RT frame ack $opId");
    }
  }

  Future<void> _incomingMessageHandler(Uint8List buf) async {
    final frameKind = buf[0].toFrameKind();
    final frameLenBuf = Uint8List.fromList(buf.getRange(1, 5).toList());
    final frameLen = frameLenBuf.buffer.asUint32List().first;
    final payload = await _soket.read(frameLen);

    var i = payload.indexOf(0);
    if (i == -1) {
      throw DataError("Invalid BUS/RT frame");
    }
    final sender = _utf8decoder.convert(payload.getRange(0, i).toList());
    final primarySender = sender.split(secondarySep).first;
    i += 1;

    String? topic;

    if (frameKind == FrameKind.publish) {
      final t = payload.skip(i).toList().indexOf(0);

      if (t == -1) {
        throw DataError("Invalid BUS/RT frame");
      }

      topic = _utf8decoder.convert(payload.getRange(i, i + t).toList());
      i += (t + 1);
    }

    final payloadPos = i;

    if (_onFrame is Function) {
      final frame = Frame(
        kind: frameKind,
        buf: payload,
        payloadPos: payloadPos,
        qos: buf[5].toQos(),
        sender: sender,
        primarySender: primarySender,
        topic: topic,
      );

      _onFrame!(frame);
    }
  }

  Future<void> _disconnect() async {
    await _soket.disconnect();
    _connected = false;
  }

  Future<void> _handleDaemonException(Object e, StackTrace s) async {
    await _connLoc.acquire();
    final connect = isConnected();
    try {
      await _disconnect();
    } finally {
      _connLoc.release();
      if (connect) {
        print({"err": e, "st": s});
        if (_onDisconnect is Function) {
          _onDisconnect!();
        }
      }
    }
  }
}
