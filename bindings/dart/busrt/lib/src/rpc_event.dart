import 'dart:convert';
import 'dart:typed_data';

import 'package:busrt/busrt.dart';

class RpcEvent {
  final RpcEventKind _kind;
  final Frame _frame;
  final int _pyloadPos;
  final Uint8List? _method;
  final int? _callId;

  RpcEvent(this._kind, this._frame, this._pyloadPos,
      [this._callId, this._method]);

  Uint8List get payload =>
      Uint8List.fromList(_frame.payload.skip(_pyloadPos).toList());

  RpcEventKind get kind => _kind;

  String? get method => _method == null ? null : Utf8Decoder().convert(_method);

  int? get callId => _callId;

  Frame? get frame => _frame;
}
