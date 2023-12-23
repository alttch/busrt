import 'package:typed_data/typed_data.dart';

const int opNop = 0x00;
const int opPublish = 0x01;
const int opSubscribe = 0x02;
const int opUnsubscribe = 0x03;
const int opMessage = 0x12;
const int opBroadcast = 0x13;
const int opAck = 0xFE;

const int protocolVersion = 0x01;

const int responseOk = 0x01;

final pingFrame = Uint8Buffer()..addAll([0, 0, 0, 0, 0, 0, 0, 0, 0]);

const errClientNotRegistered = 0x71;
const errData = 0x72;
const errIo = 0x73;
const errOther = 0x74;
const errNotSupported = 0x75;
const errBusy = 0x76;
const errNotDelivered = 0x77;
const errTimeout = 0x78;
const errAccess = 0x79;

const greetings = 0xEB;

const defaultTimeout = Duration(seconds: 5);
const defaultPingInterval = Duration(microseconds: 1);
const maxPayloadLen = 0xffffffff;

const secondarySep = "%%";
