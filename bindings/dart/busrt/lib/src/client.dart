import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:busrt/src/consts.dart';
import 'package:busrt/src/error_kind.dart';
import 'package:busrt/src/frame.dart';
import 'package:busrt/src/frame_kind.dart';
import 'package:busrt/src/future_soket.dart';
import 'package:busrt/src/op_result.dart';
import 'package:busrt/src/qos.dart';
import 'package:mutex/mutex.dart';
import 'package:typed_data/typed_data.dart';

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

  final frames = <int, OpResult>{};
  final _soket = FutureSoket();
  void Function(Frame frame)? _onFrame;
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
      Future.microtask(() => Timer.periodic(Duration.zero, _reader));
    } finally {
      _connLoc.release();
    }
  }

  bool isConnected() => _connected && _soket.isConnected();

  set onFrame(void Function(Frame frame)? fn) {
    _onFrame = fn;
  }

  set onDisconnect(void Function()? fn) {
    _onDisconnect = fn;
  }

  void _incrementFrameId() {
    if (_frameId >= 0xffffffff) {
      _frameId = 1;
    } else {
      _frameId += 1;
    }
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

    _soket.write(Uint8Buffer()
      ..addAll(nameLen)
      ..addAll(name));
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
    } catch (e) {
      t.cancel(); //TODO
    } finally {
      _sendLoc.release();
    }
  }

  Future<void> _reader(Timer t) async {
    try {
      final buf = await _soket.read(6);
      switch (buf[0].toFrameKind()) {
        case FrameKind.nop:
          return;
        case FrameKind.acknowledge:
          return _askHandler(buf);
        default:
          return await _incomingMessageHandler(buf);
      }
    } catch (e) {
      t.cancel();
      // TODO
    }
  }

  void _askHandler(Uint8Buffer buf) {
    final opIdBbuf = Uint8Buffer()..addAll(buf.getRange(1, 5));
    final opId = opIdBbuf.buffer.asUint32List().first;
    final res = frames.remove(opId);
    if (res is OpResult) {
      res.setResult(buf[5]);
      res.unloc();
    } else {
      print("orphaned BUS/RT frame ack $opId");
    }
  }

  Future<void> _incomingMessageHandler(Uint8Buffer buf) async {
    final frameKind = buf[0].toFrameKind();
    final frameLenBuf = Uint8Buffer()..addAll(buf.getRange(1, 5));
    final frameLen = frameLenBuf.buffer.asUint32List().first;
    final payload = await _soket.read(frameLen);
    
    var i = payload.indexOf(0);
    if (i == -1) {
      throw DataError("Invalid BUS/RT frame");
    }

    final sender = _utf8decoder.convert(buf.getRange(0, i).toList());
    final primarySender = sender.split(secondarySep).first;
    i += 1;
    
    String? topic;

    if (frameKind == FrameKind.publish) {

      final t = buf.skip(i).toList().indexOf(0);

      if (t == -1) {
        throw DataError("Invalid BUS/RT frame");
      }

      topic = _utf8decoder.convert(buf.getRange(i, i + t).toList());
      i = t + 1;
    }

    final payloadPos = i;

    if (_onFrame is Function) {
      final frame = Frame(
        kind: frameKind,
        buf: buf,
        payloadPos: payloadPos,
        realtime: buf[5].toQos().isRealtime(),
        sender: sender,
        primarySender: primarySender,
        topic: topic,
      );

      _onFrame!(frame);
    }
    

  }
}
