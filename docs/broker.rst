Broker
******

The broker is the central elbus instance, which routes messages between
applications. Being the broker is the only way to exchange messages between
local threads, so usually the broker is embedded into the central heaviest
application, while all plug-ins, services and additional components talk with
the broker using inter-process communications (UNIX sockets or TCP).

elbus broker is currently implemented in Rust only.

Broker API
==========

When **broker-api** feature is enabled, the following default RPC methods are
available at **.broker** automatically:

* **client.list()** - list all connected clients
* **benchmark.test(payload)** - test method, returns the payload as-is

The payload exchange format (call params / replies) is MessagePack.

Stand-alone broker server
=========================

To build a stand-alone broker server, use the command:

.. code:: shell
    
    cargo build --features server,broker-api

The *broker-api* feature is optional. When enabled, it allows to call broker
default internal functions.

Embedded broker
===============

.. note::

    If compiling for "musl" target, it is strongly recommended to replace the
    default MUSL allocator with 3rd party, e.g. with `jemallocator
    <https://crates.io/crates/jemallocator>`_ to keep the broker fast.

Example of a broker with inter-thread communications and external clients:

.. literalinclude:: ../examples/inter_thread.rs
    :language: rust
