import 'dart:io';
import 'dart:typed_data';

import 'package:typed_data/typed_data.dart';

class FutureSoket {
  Socket? _socket;
  final _buffer = Uint8Buffer();

  Future<void> connect(InternetAddress host, int port, Duration timeout) async {
    _socket = await Socket.connect(host, port, timeout: timeout);
    _socket?.listen((e) => _buffer.addAll(e),
        onError: (_) async => await disconnect(),
        onDone: () async => await disconnect());
  }

  bool isConnected() => _socket is Socket;

  Future<Uint8List> read(int len) async {
    while (_buffer.length < len) {
      if (!isConnected()) {
        throw SocketException.closed();
      }
      await Future.delayed(Duration.zero);
    }

    final buf = Uint8List.fromList(_buffer.take(len).toList());
    _buffer.removeRange(0, len);

    return buf;
  }

  void write(Uint8List buf) {
    if (!isConnected()) {
      throw SocketException.closed();
    }

    _socket!.add(buf);
  }

  Future<void> disconnect() async {
    await _socket?.flush();
    _socket?.close();
    _socket?.destroy();
    _socket = null;
  }
}
