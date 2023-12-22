import 'dart:io';

import 'package:busrt/src/consts.dart';
import 'package:busrt/src/op_result.dart';
import 'package:mutex/mutex.dart';

class Bus {
  final String _name;
  final Duration _pingInterval;
  final Duration _timeout;
  bool _connected = false;
  final _loc = Mutex();
  int _frameId = 0;
  final frames = <int, OpResult>{};
  Socket? _soket;

  Bus(this._name, {
    Duration pingInterval = defaultPingInterval,
    Duration timeout = defaultTimeout,
  }):
    _pingInterval = pingInterval,
    _timeout = timeout;

  Future<void> connect(String path) async {
    await _loc.acquire();
    try {
      final (host, port) = _hostAndPortFromPath(path);
      _soket = await Socket.connect(host, port);
    } finally {
      _loc.release();
    }
  }

  bool isConnected() => _connected;

  void _incrementFrameId() {
    if (_frameId >= 0xffffffff) {
      _frameId = 1;
    } else {
      _frameId += 1;
    }
  }

  (InternetAddress, int) _hostAndPortFromPath(String path) {
    final conditional = path.endsWith('.sock') || path.endsWith('.socket')
      || path.endsWith('.ipc') || path.startsWith('/');

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
}
