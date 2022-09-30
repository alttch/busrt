# BUS/RT - Rust-native IPC broker

<img src="https://raw.githubusercontent.com/alttch/busrt/main/docs/images/logo-dark.svg"
width="200" />

## What is BUS/RT

BUS/RT® is a rust-native IPC broker, written in Rust/Tokio, inspired by
[NATS](https://nats.io), [ZeroMQ](https://zeromq.org) and
[Nanomsg](https://nanomsg.org). BUS/RT is fast, flexible and very easy to use.

The library can be embedded in any Rust project or be used as a standalone
server.

BUS/RT is the core bus of [EVA ICS v4](https://www.eva-ics.com/)

The name "BUS/RT" is also only the working one, after the official release the
bus will be renamed, as well as its crates, bindings and modules.

## Inter-process communication

The following communication patterns are supported out-of-the-box:

* one-to-one messages
* one-to-many messages
* pub/sub

The following channels are supported:

* async channels between threads/futures (Rust only)
* UNIX sockets (local machine)
* TCP sockets

In addition to Rust, BUS/RT has also bindings for the following languages:

* Python (sync): <https://pypi.org/project/busrt/>
* Python (async): <https://pypi.org/project/busrt-async/>
* JavaScript (Node.js): <https://www.npmjs.com/package/busrt>

Rust crate: <https://crates.io/crates/busrt>

### Client registration

A client should register with a name "group.subgroup.client" (subgroups are
optional). The client's name can not start with dot (".", reserved for internal
broker clients) if registered via IPC.

The client's name must be unique, otherwise the broker refuses the
registration.

### Broadcasts

Broadcast messages are sent to multiple clients at once. Use "?" for any part
of the path, "\*" as the ending for wildcards. E.g.:

"?.test.\*" - the message is be sent to clients "g1.test.client1",
"g1.test.subgroup.client2" etc.

### Topics

Use [MQTT](https://mqtt.org)-format for topics: "+" for any part of the path,
"#" as the ending for wildcards. E.g. a client, subscribed to "+/topic/#"
receives publications sent to "x/topic/event", "x/topic/sub/event" etc.

## RPC layer

An optional included RPC layer for one-to-one messaging can be used. The layer
is similar to [JSON RPC](https://www.jsonrpc.org/) but is optimized for byte
communications.

## Security and reliability model

BUS/RT has a very simple optional security model in favor of simplicity and
speed. Also, BUS/RT is not designed to work via unstable connections, all
clients should be connected either from the local machine or using high-speed
reliable local network communications.

If you need a pub/sub server for a wide area network, try
[PSRT](https://github.com/alttch/psrt/).

## Examples

See [examples](https://github.com/alttch/busrt/tree/main/examples) folder.

## Build a stand-alone server

```ignore
cargo build --features server,rpc
```

The "rpc" feature is optional. When enabled for the server, it allows to
initialize the default broker RPC API, spawn fifo servers, send broker
announcements etc.

## Some numbers

### Benchmarks

CPU: i7-7700HQ

Broker: 4 workers, clients: 8, payload size: 100 bytes, local IPC (single unix
socket), totals:

| stage                    | iters/s     |
|--------------------------|-------------|
| rpc.call                 | 126\_824    |
| rpc.call+handle          | 64\_694     |
| rpc.call0                | 178\_505    |
| send+recv.qos.no         | 1\_667\_131 |
| send+recv.qos.processed  | 147\_812    |
| send.qos.no              | 2\_748\_870 |
| send.qos.processed       | 183\_795    |

## About the authors

[Bohemia Automation](https://www.bohemia-automation.com) /
[Altertech](https://www.altertech.com) is a group of companies with 15+ years
of experience in the enterprise automation and industrial IoT. Our setups
include power plants, factories and urban infrastructure. Largest of them have
1M+ sensors and controlled devices and the bar raises higher and higher every
day.
