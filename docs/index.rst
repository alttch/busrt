elbus - Rust-native IPC broker
==============================

What is elbus
-------------

elbus is a rust-native IPC broker, written in Rust/Tokio, inspired by
`zeromq <https://zeromq.org>`_ and `nanomsg <https://nanomsg.org>`_.
elbus is fast, flexible and very easy to use.

The library can be embedded in any Rust project or be used as a
standalone server.

Code repository: https://github.com/alttch/elbus

Inter-process communication
---------------------------

The following communication patterns are supported out-of-the-box:

-  one-to-one messages
-  one-to-many messages
-  pub/sub

The following channels are supported:

-  async channels between threads/futures (Rust only)
-  UNIX sockets (local machine)
-  TCP sockets

Crate features
--------------

* **ipc** - enable IPC client
* **rpc** - enable optional RPC layer
* **broker** - enable broker
* **full** - IPC+RPC+broker
* **server** - build stand-alone broker server
* **cli** - build CLI tool
* **std-alloc** - forcibly use the standard memory allocator for server/cli
  (enable in case of problems with jemalloc)

QoS
---

All elbus frames have 4 types of QoS:

* No (0) - does not need confirmation, not real-time
* Processed (1) - needs confirmation from the broker, not real-time
* Realtime (2) - does not need confirmation, real-time
* RealtimeProcessed (3) - needs confirmation from the broker, real-time

When real-time frames are send to a socket, its write buffer is flushed
immediately. Otherwise, a "buf_ttl" delay may occur (>1ms), unless any data is
sent after and the buffer is flushed automatically.

.. include:: readme.rst

.. toctree::
    :caption: Technical documentation
    :maxdepth: 1

    Broker <broker>
    Protocol specification <protocol>
    RPC layer specification <rpc-protocol>

.. toctree::
    :caption: Bindings
    :maxdepth: 1

    Rust client/broker <https://docs.rs/elbus>
    Python client (sync) <python/elbus>
    Python client (async) <python/elbus_async>
    Javascript client <https://www.npmjs.com/package/elbus>
