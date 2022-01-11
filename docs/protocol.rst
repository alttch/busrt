elbus protocol specification
****************************

## greetings

server: EB 01 00

client: EB 01 00

server: 01 or 75 if not supported and closes

client: XX XX (len) ID (string-utf8-bytes)

server: 01 (OK) or XX (error code) and closes the connection

## outgoing frames

client: XX XX XX XX (OP-ID-CUSTOM) FLAGS XX XX XX XX (frame len) TARGET 00 PAYLOAD

FLAGS = u8, first 6 bits - op, last 2 bits = QoS (0 - no confirm, 1 - confirm
processed)

Operations:

0 - NOP
1 - publish to topic, target = topic
2 - subscribe to topic(s), no target required
3 - unsubscribe from topic(s), no target required
0x12 - direct message
0x13 - broadcast message

## pings

client: XX XX XX XX 00 XX XX XX XX

where XX - any bytes (so 0x00 * 9 is fine)

the client should ping the server to make sure the connection is alive. the
server may ping the client, no reply is required.

## incoming frames

first 6 bytes:

0 - frame type
1 - frame len or op id
2
3
4
5 - reserved or ack result

## acknowledgements

The server sends acks for all operations with QoS > 0

server: FE XX XX XX XX (OP-ID-CUSTOM) 01 (OK) or error code

## messages

server: OP XX XX XX XX 00 (frame len) SENDER 00 PAYLOAD 

## topics

server: OP XX XX XX XX 00 (frame len) SENDER 00 TOPIC 00 PAYLOAD 

topics starting with "!bus" are considered as system and all clients are
subscribed to them automatically.
