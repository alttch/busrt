import 'dart:io';

import 'package:typed_data/typed_data.dart';

class FutureSoket {
  Socket? _socket;
  final _buffer = Uint8Buffer();

  Future<void> connect(InternetAddress host, int port, Duration timeout) async {
    _socket = await Socket.connect(host, port, timeout: timeout);
    _socket?.listen((e) { _buffer.addAll(e); });
    _socket?.drain().then((_) { _socket = null; });
  }

  bool isConnected() => _socket is Socket;

  Future<Uint8Buffer> read(int len) async {
    while (_buffer.length < len) {
      await Future.delayed(Duration.zero);
    }

    final buf = Uint8Buffer()..addAll(_buffer.take(len));
    _buffer.removeRange(0, len);

    return buf;
  }

  void write(Uint8Buffer buf) {
    if (!isConnected()) {
      throw SocketException.closed();
    }

    _socket!.write(buf);
  }
}
