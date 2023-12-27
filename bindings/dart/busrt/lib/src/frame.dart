import 'dart:convert';

import 'package:busrt/src/frame_kind.dart';
import 'package:busrt/src/qos.dart';
import 'package:busrt/src/rpc_event_kind.dart';
import 'package:typed_data/typed_data.dart';

class Frame {
  final FrameKind _kind;
  final String? _sender;
  final String? _primarySender;
  final String? _topic;
  final Uint8Buffer? _header;
  final Uint8Buffer _buf;
  int _payloadPos;
  final QoS _qos;

  Frame({
    required FrameKind kind,
    required QoS qos,
    Uint8Buffer? buf,
    int payloadPos = 0,
    String? sender,
    String? primarySender,
    String? topic,
    Uint8Buffer? header,
  })  : _kind = kind,
        _buf = buf ?? Uint8Buffer(),
        _payloadPos = payloadPos,
        _qos = qos,
        _sender = sender,
        _primarySender = primarySender,
        _topic = topic,
        _header = header;

  Frame.rpcNotification({
    Uint8Buffer? payload,
    QoS qos = QoS.processed,
  })  : _kind = FrameKind.message,
        _qos = qos,
        _buf = payload ?? Uint8Buffer(),
        _header = Uint8Buffer()..add(RpcEventKind.notification.value),
        _sender = null,
        _primarySender = null,
        _topic = null,
        _payloadPos = 0;

  Frame.rpcRequest({
    required String method,
    required Uint8Buffer? params,
    QoS qos = QoS.processed,
    Iterable<int> callId = const [0, 0, 0, 0],
  })  : _kind = FrameKind.message,
        _qos = qos,
        _buf = params ?? Uint8Buffer(),
        _header = Uint8Buffer()
          ..add(RpcEventKind.request.value)
          ..addAll(callId)
          ..addAll(Utf8Encoder().convert(method))
          ..addAll(Uint8Buffer(1)),
        _sender = null,
        _primarySender = null,
        _topic = null,
        _payloadPos = 0;

  Frame.rpcReplay({
    Uint8Buffer? result,
    QoS qos = QoS.processed,
    Uint8Buffer? header,
  }) :
    _kind = FrameKind.message,
    _buf = result ?? Uint8Buffer(),
    _qos = qos,
    _header = header,
    _sender = null,
    _primarySender = null,
    _topic = null,
    _payloadPos = 0
  ;

  FrameKind get kind => _kind;

  String? get sender => _sender;

  String? get primarySender => _primarySender;

  String? get topic => _topic;

  Uint8Buffer get payload => Uint8Buffer()..addAll(_buf.skip(_payloadPos));

  Uint8Buffer? get header => _header;

  QoS get qos => _qos;

  void addPayloadPos(int add) => _payloadPos += add;
}
