import 'package:busrt/src/consts.dart';
import 'package:busrt/src/error_kind.dart';

enum FrameKind {
  prepared(0xff),
  message(opMessage),
  broadcast(opBroadcast),
  publish(opPublish),
  acknowledge(opAck),
  nop(opNop);

  final int value;

  const FrameKind(this.value);
}

extension ToFrameKind on int {
  FrameKind toFrameKind() => switch (this) {
        opMessage => FrameKind.message,
        opBroadcast => FrameKind.broadcast,
        opPublish => FrameKind.publish,
        opAck => FrameKind.acknowledge,
        opNop => FrameKind.nop,
        _ => throw DataError("Invalid frame type: $this"),
      };
}
