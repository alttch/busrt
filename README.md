<h2>
  BUS/RT - Rust-native IPC broker
  <a href="https://crates.io/crates/busrt"><img alt="crates.io page" src="https://img.shields.io/crates/v/busrt.svg"></img></a>
  <a href="https://docs.rs/busrt"><img alt="docs.rs page" src="https://docs.rs/busrt/badge.svg"></img></a>
  <a href="https://github.com/alttch/busrt/actions/workflows/ci.yml">
    <img alt="GitHub Actions CI" src="https://github.com/alttch/busrt/actions/workflows/ci.yml/badge.svg"></img>
  </a>
</h2>


<img src="https://raw.githubusercontent.com/alttch/busrt/main/images/logo-dark.svg"
width="200" />

## What is BUS/RT

BUS/RT® is a Rust-native IPC broker, written in Rust/Tokio, inspired by
[NATS](https://nats.io), [ZeroMQ](https://zeromq.org) and
[Nanomsg](https://nanomsg.org). BUS/RT is fast, flexible and very easy to use,
optimized for both high-load and ultra-low latency real-time scenarios.

The library can be embedded in any Rust project or be used as a standalone
server.

BUS/RT is the core bus of [EVA ICS v4](https://www.eva-ics.com/).

## Inter-process communication

The following communication patterns are supported out-of-the-box:

* one-to-one messages
* one-to-many messages
* pub/sub

The following channels are supported:

* async channels between threads/futures (Rust only)
* UNIX sockets (local machine, Linux/BSD)
* TCP sockets (Linux/BSD/Windows)

In addition to Rust, BUS/RT has also bindings for the following languages:

* Python (sync): <https://pypi.org/project/busrt/>
* Python (async): <https://pypi.org/project/busrt-async/>
* JavaScript (Node.js): <https://www.npmjs.com/package/busrt>
* Dart <https://github.com/AndreiLosev/busrt_client>

Rust crate: <https://crates.io/crates/busrt>

## Real-time safety

Use `rt` feature to use for internal mutexes
[`parking_lot_rt`](https://crates.io/crates/parking_lot_rt) - a `parking_lot`
fork without spin-locks, which is real-time safe.

## Technical documentation

The full documentation is available at: <https://info.bma.ai/en/actual/busrt/>

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
