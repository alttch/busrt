import 'package:busrt/src/bus_error.dart';

enum RpcEventKind {
  notification(0x00),
  request(0x01),
  reply(0x11),
  error(0x12);

  final int value;

  const RpcEventKind(this.value);
}

extension ToRpcEventKind on int {
  RpcEventKind toRpcEventKind() => switch (this) {
        0x00 => RpcEventKind.notification,
        0x01 => RpcEventKind.request,
        0x11 => RpcEventKind.reply,
        0x12 => RpcEventKind.error,
        _ => throw DataError("Invalid Rpc event type: $this"),
      };
}
