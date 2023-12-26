part of "bus.dart";

class Rpc {
  final Bus _bus;
  int _callId = 0;
  final _callLock = Mutex();
  final _calls = <int, RpcOpResult>{};

  FutureOr<void> Function(RpcEvent e)? _onEvent;
  FutureOr<void> Function(RpcEvent e)? _onNotification;
  FutureOr<Uint8Buffer?> Function(RpcEvent e) _onCall =
      (_) => throw RpcMethodNotFoundError("RPC engine not intialized");

  final bool _blockingNotifications;
  final bool _blockingFrames;

  Rpc(this._bus,
      {bool blockingFrames = false, bool blockingNotifications = false})
      : _blockingFrames = blockingFrames,
        _blockingNotifications = blockingNotifications {
    _bus.onFrame = _handleFrame;
  }

  bool isConnected() => _bus.isConnected();

  Future<OpResult> notify(String target,
      {Uint8Buffer? payload, QoS qos = QoS.processed}) async {
    final notification = Frame.rpcNotification(
      qos: qos,
      payload: payload,
    );

    return await _bus._send(notification, [target]);
  }

  Future<OpResult> call0(String target, String method,
      {Uint8Buffer? params, QoS qos = QoS.processed}) async {
    final request = Frame.rpcRequest(method: method, params: params, qos: qos);

    return await _bus._send(request, [target]);
  }

  Future<RpcOpResult> call(String target, String method,
      {Uint8Buffer? params, QoS qos = QoS.processed}) async {
    await _callLock.acquire();
    final callId = _incrimentCalId();
    _callLock.release();
    final rpcResult = RpcOpResult();
    await rpcResult.loc();
    _calls[callId] = rpcResult;
    final callIdBuf = Uint32List.fromList([callId]).buffer.asInt8List();

    final request = Frame.rpcRequest(
        method: method, params: params, qos: qos, callId: callIdBuf);

    try {
      final opc = await _bus._send(request, [target]);
      try {
        await opc.waitCompleted();
      } on ErrorKind catch (e) {
        _calls.remove(callId);
        rpcResult.err = e;
        rpcResult.unloc();
      }
    } catch (e) {
      _calls.remove(callId);
      rpcResult.err = IoError(e.toString());
      rpcResult.unloc();
    }

    return rpcResult;
  }

  Future<void> _handleFrame(Frame frame) async {
    if (frame.kind != FrameKind.message && _onEvent != null) {
      final e = RpcEvent(RpcEventKind.request, frame, 1);
      if (_blockingFrames) {
        await _onEvent!(e);
      } else {
        Future.microtask(() => _onEvent!(e));
      }
    }

    final eventKind = frame.payload[0].toRpcEventKind();

    switch (eventKind) {
      case RpcEventKind.notification:
        await _notificationHandle(frame);
      case RpcEventKind.request:
        await _requestHandle(frame);
      case RpcEventKind.reply || RpcEventKind.error:
        _replyOrErrHandle(frame, eventKind);
      default:
        throw RpcCodeNotFoundError("Invalid RPC frame code $eventKind");
    }

    throw UnimplementedError();
  }

  Future<void> _notificationHandle(Frame frame) async {
    if (_onNotification == null) {
      return;
    }

    final e = RpcEvent(frame.payload[0].toRpcEventKind(), frame, 1);

    if (_blockingNotifications) {
      await _onNotification!(e);
    } else {
      Future.microtask(() => _onNotification!(e));
    }
  }

  Future<void> _requestHandle(Frame frame) async {
    final sender = frame.sender;
    final callIdBuf = Uint8List.fromList(
      frame.payload.getRange(1, 5).toList(),
    );
    final callId = callIdBuf.buffer.asUint32List().first;

    final s = frame.payload.take(5).toList();
    int i = s.indexOf(0);
    if (i == -1) {
      throw DataError("Invalid BUS/RT frame");
    }

    final method = Uint8Buffer()..addAll(s.getRange(0, i));
    final e = RpcEvent(
      RpcEventKind.request,
      frame,
      6 + method.length,
      callId,
      method,
    );

    if (callId == 0) {
      await _onCall(e);
      return;
    }

    late final Frame replay;
    try {
      final buffer = (await _onCall(e)) ?? Uint8Buffer(1);
      final header = Uint8Buffer()
        ..add(RpcEventKind.reply.value)
        ..addAll(callIdBuf);
      replay = Frame.rpcReplay(
        result: buffer,
        header: header,
        qos: frame.qos,
      );
    } catch (e) {
      final code = e is ErrorKind ? e.value : RpcInternalError().value;
      final codeBuf = Uint8Buffer()
        ..addAll(Int16List.fromList([code]).buffer.asUint8List());
      final header = Uint8Buffer()
        ..add(RpcEventKind.reply.value)
        ..addAll(callIdBuf)
        ..addAll(codeBuf);
      replay = Frame.rpcReplay(
        header: header,
        qos: frame.qos,
      );
    }

    if (sender is String) {
      await _bus._send(replay, [sender]);
    }
  }

  void _replyOrErrHandle(Frame frame, RpcEventKind rpcKind) {
    final callId = Uint8List.fromList(frame.payload.skip(1).take(4).toList())
        .buffer
        .asUint32List()
        .first;

    final result = _calls[callId];

    if (result == null) {
      print("orphaned RPC response: $callId");
      return;
    }

    _calls.remove(callId);
    result.frame = frame;
    if (rpcKind == RpcEventKind.error) {
      final eCode = Uint8List.fromList(frame.payload.skip(5).take(4).toList())
          .buffer
          .asInt16List()
          .first;

      result.err = eCode
          .toErrKind(_bus._utf8decoder.convert(frame.payload.skip(7).toList()));
    }
    result.unloc();
  }

  int _incrimentCalId() {
    if (_callId >= 0xffffffff) {
      _callId = 0;
    } else {
      _callId += 1;
    }

    return _callId;
  }
}
