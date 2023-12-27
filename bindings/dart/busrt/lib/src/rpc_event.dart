import 'dart:convert';

import 'package:busrt/busrt.dart';
import 'package:typed_data/typed_data.dart';

class RpcEvent {
  final RpcEventKind _kind;
  final Frame _frame;
  final int _pyloadPos;
  final Uint8Buffer? _method;
  final int? _callId;

  RpcEvent(this._kind, this._frame, this._pyloadPos,
      [this._callId, this._method]);

  Uint8Buffer get payload =>
      Uint8Buffer()..addAll(_frame.payload.skip(_pyloadPos));

  RpcEventKind get kind => _kind;

  String? get method => _method == null ? null : Utf8Decoder().convert(_method);

  int? get callId => _callId;

  Frame? get frame => _frame;
}
