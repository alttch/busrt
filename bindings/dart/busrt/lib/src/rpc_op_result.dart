import 'package:busrt/busrt.dart';
import 'package:busrt/src/error_kind.dart';
import 'package:mutex/mutex.dart';

class RpcOpResult {
  final _completed = Mutex();
  ErrorKind? err;
  Frame? frame;

  Future<void> loc() async => await _completed.acquire();

  void unloc() => _completed.release();
}
