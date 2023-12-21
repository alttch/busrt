import 'package:busrt/src/consts.dart';
import 'package:busrt/src/bus_error.dart';

enum FrameKind {
  message(opMessage),
  broadcast(opBroadcast),
  publish(opPublish),
  acknowledge(opAck),
  subscribeTopic(opSubscribe),
  unsubscribeTopic(opUnsubscribe),
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
        opSubscribe => FrameKind.subscribeTopic,
        opUnsubscribe => FrameKind.unsubscribeTopic,
        _ => throw DataError("Invalid frame type: $this"),
      };
}
