# elbus - Rust-native IPC broker

<img src="https://raw.githubusercontent.com/alttch/elbus/main/docs/images/logo-dark.svg"
width="200" />

<https://elbus.bma.ai/>

## What is elbus

elbus is a rust-native IPC broker, written in Rust/Tokio, inspired by
[zeromq](https://zeromq.org) and [nanomsg](https://nanomsg.org). elbus is fast,
flexible and very easy to use.

The library can be embedded in any Rust project or be used as a standalone
server.

NOTE: elbus is not officially released yet and SDK can be changed at any time
without any backward compatibility. elbus (stands actually for "EVA ICS Local
Bus") is the key component for [EVA ICS](https://www.eva-ics.com/) v4, which is
going to be released in 2022-2023.

## Documentation

Available at <https://elbus.readthedocs.io/>

## Inter-process communication

The following communication patterns are supported out-of-the-box:

* one-to-one messages
* one-to-many messages
* pub/sub

The following channels are supported:

* async channels between threads/futures (Rust only)
* UNIX sockets (local machine)
* TCP sockets

In addition to Rust, elbus has also bindings for the following languages:

* Python (sync): <https://pypi.org/project/elbus/>
* Python (async): <https://pypi.org/project/elbus-async/>
* JavaScript (Node.js): <https://www.npmjs.com/package/elbus>

Rust crate: <https://crates.io/crates/elbus>

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

elbus has ZERO security model in favor of simplicity and speed. Also, elbus is
not designed to work via unstable connections, all clients should be connected
either from the local machine or using high-speed reliable local network
communications.

If you need a pub/sub server for a wide area network, try
[PSRT](https://github.com/alttch/psrt/).

## Examples

See [examples](https://github.com/alttch/elbus/tree/main/examples) folder.

## Build a stand-alone server

```
cargo build --features server,rpc
```

The "rpc" feature is optional. When enabled for the server, it allows to
initialize the default broker RPC API, spawn fifo servers, send broker
announcements etc.

## Some numbers

CPU: AMD Ryzen 9 5950X

Broker: 8 workers, clients: 10, payload size: 500 bytes, local IPC (single unix
socket), totals:

```shell
elbus /tmp/elbus.sock benchmark -w10 --payload-size 500
```

| stage                    | iters/s     |
|--------------------------|-------------|
| rpc.call                 | 358\_513    |
| rpc.call+handle          | 187\_043    |
| rpc.call0                | 446\_663    |
| send+recv.qos.no         | 478\_062    |
| send+recv.qos.processed  | 375\_817    |
| send.qos.no              | 3\_019\_431 |
| send.qos.processed       | 446\_416    |

According to tests, elbus, with all its functionality, is minimum 3-5 times
FASTER than any simple HTTP RPC/notification service
send.qos.processed/rpc.call benchmarks (e.g. [simple
RPC](https://gist.github.com/divi255/e166673c5bc8cb833456a0acf6d951bf) build
with one of the fastest, but still HTTP framework [Hyper](https://hyper.rs)).

Handler benchmarks over HTTP usually require an additional abstraction layer
(e.g. web sockets), which makes things even more worse.

Comparison of "qos.no" stages is useless, as HTTP always must return some
response, even if it is has no content.

## About the authors

[Bohemia Automation](https://www.bohemia-automation.com) /
[Altertech](https://www.altertech.com) is a group of companies with 15+ years
of experience in the enterprise automation and industrial IoT. Our setups
include power plants, factories and urban infrastructure. Largest of them have
1M+ sensors and controlled devices and the bar raises upper and upper every
day.
