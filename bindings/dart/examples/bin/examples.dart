import 'dart:isolate';
import 'dart:typed_data';

import 'package:busrt/busrt.dart';
import 'package:msgpack_dart/msgpack_dart.dart';

int myValue = 0;
int fromRpcCall = 0;
int sum = 0;

void main(List<String> arguments) async {
  
  final start = DateTime.now();

  final f1 = Isolate.run(() => runWorker('worker.add', 'worker.minus'));
  final f2 = Isolate.run(() => runWorker('worker.minus', 'worker.add'));

  final [res1, res2] = await Future.wait([f1, f2]);

  print([res1, res2, DateTime.now().difference(start)]);
}

Future<Map<String, int>> runWorker(String myName, String parnterName) async {
    final bus = Bus(myName);
    final rpc = Rpc(bus, onCall: onCall, onNotification: ononNotification);
    await rpc.bus.connect("/tmp/busrt.sock");
    final method = parnterName.replaceAll('worker.', '');
    final count = method == 'add' ? 501 : 499;
    await Future.delayed(Duration(milliseconds: 10));
    for (var i in List.generate(count, (i) => i)) {
      final res = await rpc.call(parnterName, method, params: serialize({'value': i}));
      await res.waitCompleted();
    }

    await Future.delayed(Duration(milliseconds: 10));

    final res = await rpc.call(parnterName, 'get');
    fromRpcCall = deserialize((await res.waitCompleted())!.payload)['value'];
    
    final r = await rpc.notify(parnterName, payload: serialize({'value': myValue}));
    await r.waitCompleted();

    await Future.delayed(Duration(milliseconds: 10));

    final stat = await rpc.call('.broker', 'stats');
    final frame = await stat.waitCompleted();
    print(deserialize(frame!.payload));

    await rpc.bus.disconnect();
  
    return {"myValue": myValue, "fromRpcCall": fromRpcCall, "sum": sum};
}

void ononNotification(RpcEvent e) {
  sum = myValue + deserialize(e.payload)['value'] as int;
}

Uint8List? onCall(RpcEvent e) {
  final method = e.method;

  switch (method) {
    case 'add':
      final payload = deserialize(e.payload)['value'] as int;
      myValue += payload;
      return null;
    case 'minus':
      final payload = deserialize(e.payload)['value'] as int;
      myValue -= payload;
      return null;
    case 'get':
      return serialize({'value': myValue});
    default:
      throw RpcMethodNotFoundError(method);
  }
}
