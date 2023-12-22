import 'package:busrt/src/qos.dart';

class OpResult {
  final QoS _qos;
  int? _result;

  OpResult(this._qos);

  set result(int? v) {
    _result = v;
  }

  Future<int?> waitCompletedCode() async {
    // TODO
    throw UnimplementedError();
  }
}
