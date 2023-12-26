import 'package:busrt/src/error_kind.dart';
import 'package:busrt/src/qos.dart';
import 'package:mutex/mutex.dart';

class OpResult {
  final QoS _qos;
  final _loc = Mutex();
  late final ErrorKind? _result;

  OpResult(this._qos);

  void setResult(int r) {
    _result = r.toErrKind("Ask Err");
  }

  Future<void> loc() async => await _loc.acquire();

  void unloc() => _loc.release();

  Future<void> waitCompleted() async {
    if (!_qos.needsAck()) {
      return;
    }
      await _loc.acquire();
      _loc.release();
      
    if (_result != null) {
      throw _result;
    }
  }
}
