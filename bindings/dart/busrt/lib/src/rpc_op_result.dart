import 'package:busrt/busrt.dart';
import 'package:mutex/mutex.dart';

class RpcOpResult {
  final _completed = Mutex();
  BusError? _err;
  Frame? _frame;

  Future<void> loc() async => await _completed.acquire();

  void unloc() => _completed.release();

  set err(BusError? e) => _err = e;

  set frame(Frame? f) => _frame = f;

  Future<Frame?> waitCompleted() async {
    await _completed.acquire();
    _completed.release();
    
    if (_err is BusError) {
      throw _err!;
    }

    _frame?.addPayloadPos(5);
    return _frame;
  }
}
