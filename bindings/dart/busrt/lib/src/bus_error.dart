import 'package:busrt/src/consts.dart';

sealed class BusError implements Exception {
  final String? message;

  BusError([this.message]);

  int get value;

  static BusError fromInt(int code, [String? message]) => switch (code) {
        errClientNotRegistered => NotRegisteredError(message),
        errNotSupported => NotSupportedError(message),
        errIo => IoError(message),
        errTimeout => TimeoutError(message),
        errData => DataError(message),
        errBusy => BusyError(message),
        errNotDelivered => NotDeliveredError(message),
        errAccess => AccessError(message),
        0xff => EofError(message),
        errRpcParse => RpcParseError(message),
        errRpcInvalidRequest => RpcInvalidRequestError(message),
        errRpcMethodNotFound => RpcMethodNotFoundError(message),
        errRpcInvalidMethodParams => RpcInvalidMethodParamsError(message),
        errRpcInternal => RpcInternalError(message),
        rpcErrorCodeNotFound => RpcCodeNotFoundError(message),
        _ => OtherError(message),
      };

  @override
  String toString() {
    if (message is String) {
      return ": $message";
    }

    return '';
  }
}

class NotRegisteredError extends BusError {
  NotRegisteredError([super.message]);

  @override
  int get value => errClientNotRegistered;

  @override
  String toString() => "Client not registered${super.toString()}";
}

class NotSupportedError extends BusError {
  NotSupportedError([super.message]);

  @override
  int get value => errNotSupported;

  @override
  String toString() => "Feature not supported${super.toString()}";
}

class IoError extends BusError {
  IoError([super.message]);

  @override
  int get value => errIo;

  @override
  String toString() => "I/O Error${super.toString()}";
}

class TimeoutError extends BusError {
  TimeoutError([super.message]);

  @override
  int get value => errTimeout;

  @override
  String toString() => "Timeout${super.toString()}";
}

class DataError extends BusError {
  DataError([super.message]);

  @override
  int get value => errData;

  @override
  String toString() => "Data Error${super.toString()}";
}

class BusyError extends BusError {
  BusyError([super.message]);

  @override
  int get value => errBusy;

  @override
  String toString() => "Busy${super.toString()}";
}

class NotDeliveredError extends BusError {
  NotDeliveredError([super.message]);

  @override
  int get value => errNotDelivered;

  @override
  String toString() => "Frame not delivered${super.toString()}";
}

class AccessError extends BusError {
  AccessError([super.message]);

  @override
  int get value => errAccess;

  @override
  String toString() => "Access denied${super.toString()}";
}

class OtherError extends BusError {
  OtherError([super.message]);

  @override
  int get value => errOther;

  @override
  String toString() => "Othe Error${super.toString()}";
}

class EofError extends BusError {
  EofError([super.message]);

  @override
  int get value => 0xff;

  @override
  String toString() => "Eof${super.toString()}";
}

class RpcParseError extends BusError {
  RpcParseError([super.message]);

  @override
  int get value => errRpcParse;

  @override
  String toString() => "errRpcParse${super.toString()}";
}

class RpcInvalidRequestError extends BusError {
  RpcInvalidRequestError([super.message]);

  @override
  int get value => errRpcInvalidRequest;

  @override
  String toString() => "errRpcInvalidRequest${super.toString()}";
}

class RpcMethodNotFoundError extends BusError {
  RpcMethodNotFoundError([super.message]);

  @override
  int get value => errRpcMethodNotFound;

  @override
  String toString() => "errRpcMethodNotFound${super.toString()}";
}

class RpcInvalidMethodParamsError extends BusError {
  RpcInvalidMethodParamsError([super.message]);

  @override
  int get value => errRpcInvalidMethodParams;

  @override
  String toString() => "errRpcInvalidMethodParams${super.toString()}";
}

class RpcInternalError extends BusError {
  RpcInternalError([super.message]);

  @override
  int get value => errRpcInternal;

  @override
  String toString() => "errRpcInternal${super.toString()}";
}

class RpcCodeNotFoundError extends BusError {
  RpcCodeNotFoundError([super.message]);

  @override
  int get value => rpcErrorCodeNotFound;

  @override
  String toString() => "rpcErrorCodeNotFound${super.toString()}";
}

extension IntoBusRtResult on int {
  BusError? toErrKind([mesage]) {
    if (this == responseOk) {
      return null;
    }

    return BusError.fromInt(this, mesage);
  }
}
