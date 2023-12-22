import 'dart:typed_data';

import 'package:busrt/src/consts.dart';
import 'package:busrt/src/frame_kind.dart';


class Frame {
  final FrameKind _kind;
  final String? _sender;
  final String? _topic;
  final Uint8List? _header;
  final Uint8List _buf;
  final int _payloadPos;
  final bool _realtime;

  Frame(
    this._kind,
    this._buf,
    this._payloadPos,
    this._realtime, [
    this._sender,
    this._topic,
    this._header,
  ]);

  Frame.pop()
      : _kind = FrameKind.nop,
        _sender = null,
        _topic = null,
        _header = null,
        _buf = Uint8List(0),
        _payloadPos = 0,
        _realtime = false;

  FrameKind get kind => _kind;

  String get sender => _sender!;

  String primarySender() {
    final index = _sender!.indexOf(secondarySep);
    if (index == -1) {
      return _sender;
    }

    return _sender.substring(0, index);
  }

  String? get topic => _topic;

  Uint8List get payload => Uint8List.sublistView(_buf, _payloadPos);

  Uint8List? get header => _header;

  bool isRealtime() => _realtime;
}
