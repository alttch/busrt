import 'package:busrt/src/consts.dart';
import 'package:busrt/src/error_kind.dart';

enum FrameOp {
  nop(opNop),
  message(opMessage),
  broadcast(opBroadcast),
  publishTopic(opPublish),
  subscribeTopic(opSubscribe),
  unsubscribeTopic(opUnsubscribe);

  final int value;

  const FrameOp(this.value);
}

extension ToFrameOp on int {
  FrameOp toFrameOp() => switch (this) {
    opNop => FrameOp.nop,
    opMessage => FrameOp.message,
    opBroadcast => FrameOp.broadcast,
    opPublish => FrameOp.publishTopic,
    opSubscribe => FrameOp.subscribeTopic,
    opUnsubscribe => FrameOp.unsubscribeTopic,
    _ => throw DataError("Invalid frame type: $this"),
  };
}
