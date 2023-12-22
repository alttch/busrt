import 'package:busrt/src/error_kind.dart';

enum QoS {
  no(0),
  processed(1),
  realtime(2),
  realtimeProcessed(3);

  final int value;

  const QoS(this.value);

  bool isRealtime() => (value & 0x2) != 0;

  bool needsAck() => (value & 0x1) != 0;
}

extension ToQos on int {
  QoS toQos() => switch (this) {
    0 => QoS.no,
    1 => QoS.processed,
    2 => QoS.realtime,
    3 => QoS.realtimeProcessed,
    _ => throw DataError("Invalid QoS: $this"),
  };
}
