ELBUS RPC layer specification
*****************************

.. contents::

A client can send and receive notifications, incoming calls and replies.

The layer completely replaces one-to-one messaging, all notifications must be
prefixed with 0x00 to tell the client that the frame is notification-only
event.

RPC calls can be also used as notifications. If call ID is set to zero, no
response is required from the other side.

The RPC layer is similar to `JSON RPC 2.0 <https://www.jsonrpc.org>`_, but
optimized for the byte protocol.

Payload bytes
=============

0 - event type
(0x0 - notification, 0x1 - request, 0x11 - reply, 0x12 - error reply)

for event:
1 - payload

Requests
--------

1-4 call ID
5- - method (str) 00 PARAMS

The parameters can be serialized in any way, msgpack is preferred. If call ID
is zero - no response is required.

Responses
---------

1-4 call ID
5- - response payload

for error response
1-4 call ID
5-6 - error code
7 - error payload

A response payload and error can be serialized in any way, for the payload
msgpack is preferred. for the error - str

When RPC layer is on, all messages are processed as RPC or event calls,
broadcasts and topics are processed as-is.

Error codes
-----------

The standard codes are sent as i16, in the format "-32000 - ELBUS_ERROR_CODE".
