import 'dart:typed_data';

import 'package:busrt/src/consts.dart';
import 'package:busrt/src/frame_kind.dart';
import 'package:typed_data/typed_data.dart';


class Frame {
  final FrameKind _kind;
  final String? _sender;
  final String? _primarySender;
  final String? _topic;
  final Uint8Buffer? _header;
  final Uint8Buffer _buf;
  final int _payloadPos;
  final bool _realtime;

  Frame({
    required FrameKind kind,
    required Uint8Buffer buf,
    required int payloadPos,
    bool realtime = false,
    String? sender,
    String? primarySender,
    String? topic,
    Uint8Buffer? header,
  }) :
    _kind = kind,
    _buf = buf,
    _payloadPos = payloadPos,
    _realtime = realtime,
    _sender = sender,
    _primarySender = primarySender,
    _topic = topic,
    _header = header;

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

  Uint8Buffer get payload => Uint8Buffer()..addAll(_buf.skip(_payloadPos));

  Uint8Buffer? get header => _header;

  bool isRealtime() => _realtime;
}
