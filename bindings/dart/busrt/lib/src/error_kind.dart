import 'package:busrt/src/consts.dart';

sealed class ErrorKind implements Exception {
  
  final String? message;

  ErrorKind([this.message]);

  int get value;

  static ErrorKind fromInt(int code, [String? message]) => switch (code) {
    errClientNotRegistered => NotRegisteredError(message),
    errNotSupported => NotSupportedError(message),
    errIo => IoError(message),
    errTimeout => TimeoutError(message),
    errData => DataError(message),
    errBusy => BusyError(message),
    errNotDelivered => NotDeliveredError(message),
    errAccess => AccessError(message),
    0xff => EofError(message),
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

class NotRegisteredError extends ErrorKind {

  NotRegisteredError([super.message]);
  
  @override
  int get value => errClientNotRegistered;

  @override
  String toString() => "Client not registered${super.toString()}";
}

class NotSupportedError extends ErrorKind {

  NotSupportedError([super.message]);

  @override
  int get value => errNotSupported;

  @override
  String toString() => "Feature not supported${super.toString()}";

}

class IoError extends ErrorKind {

  IoError([super.message]);

  @override
  int get value => errIo;

  @override
  String toString() => "I/O Error${super.toString()}";
}

class TimeoutError extends ErrorKind {

  TimeoutError([super.message]);

  @override
  int get value => errTimeout;

  @override
  String toString() => "Timeout${super.toString()}";
}

class DataError extends ErrorKind {

  DataError([super.message]);

  @override
  int get value => errData;

  @override
  String toString() => "Data Error${super.toString()}";
}

class BusyError extends ErrorKind {

  BusyError([super.message]);

  @override
  int get value => errBusy;

  @override
  String toString() => "Busy${super.toString()}";
}

class NotDeliveredError extends ErrorKind {

  NotDeliveredError([super.message]);

  @override
  int get value => errNotDelivered;

  @override
  String toString() => "Frame not delivered${super.toString()}";
}

class AccessError extends ErrorKind {

  AccessError([super.message]);

  @override
  int get value => errAccess;

  @override
  String toString() => "Access denied${super.toString()}";
}

class OtherError extends ErrorKind {

  OtherError([super.message]);

  @override
  int get value => errOther;

  @override
  String toString() => "Othe Error${super.toString()}";
}

class EofError extends ErrorKind {

  EofError([super.message]);

  @override
  int get value => 0xff;

  @override
  String toString() => "Eof${super.toString()}";
}

extension IntoBusRtResult on int {
  ErrorKind? toBusrtResult() {
    if (this == responseOk) {
      return null;
    }

    return ErrorKind.fromInt(this);
  }
}
