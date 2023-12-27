import 'dart:convert';
import 'dart:typed_data';

import 'package:busrt/src/frame_kind.dart';
import 'package:busrt/src/qos.dart';
import 'package:busrt/src/rpc_event_kind.dart';

class Frame {
  final FrameKind _kind;
  final String? _sender;
  final String? _primarySender;
  final String? _topic;
  final Uint8List? _header;
  final Uint8List _buf;
  int _payloadPos;
  final QoS _qos;

  Frame({
    required FrameKind kind,
    required QoS qos,
    Uint8List? buf,
    int payloadPos = 0,
    String? sender,
    String? primarySender,
    String? topic,
    Uint8List? header,
  })  : _kind = kind,
        _buf = buf ?? Uint8List(0),
        _payloadPos = payloadPos,
        _qos = qos,
        _sender = sender,
        _primarySender = primarySender,
        _topic = topic,
        _header = header;

  Frame.rpcNotification({
    Uint8List? payload,
    QoS qos = QoS.processed,
  })  : _kind = FrameKind.message,
        _qos = qos,
        _buf = payload ?? Uint8List(0),
        _header = Uint8List.fromList([RpcEventKind.notification.value]),
        _sender = null,
        _primarySender = null,
        _topic = null,
        _payloadPos = 0;

  Frame.rpcRequest({
    required String method,
    required Uint8List? params,
    QoS qos = QoS.processed,
    Iterable<int> callId = const [0, 0, 0, 0],
  })  : _kind = FrameKind.message,
        _qos = qos,
        _buf = params ?? Uint8List(0),
        _header = Uint8List.fromList([
          RpcEventKind.request.value,
          ...callId,
          ...Utf8Encoder().convert(method),
          0
        ]),
        _sender = null,
        _primarySender = null,
        _topic = null,
        _payloadPos = 0;

  Frame.rpcReplay({
    Uint8List? result,
    QoS qos = QoS.processed,
    Uint8List? header,
  })  : _kind = FrameKind.message,
        _buf = result ?? Uint8List(0),
        _qos = qos,
        _header = header,
        _sender = null,
        _primarySender = null,
        _topic = null,
        _payloadPos = 0;

  FrameKind get kind => _kind;

  String? get sender => _sender;

  String? get primarySender => _primarySender;

  String? get topic => _topic;

  Uint8List get payload => Uint8List.fromList(_buf.skip(_payloadPos).toList());

  Uint8List? get header => _header;

  QoS get qos => _qos;

  void addPayloadPos(int add) => _payloadPos += add;
}
