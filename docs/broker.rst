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

When **rpc** feature is enabled, the following default RPC methods are
available at **.broker** after *broker.init_default_core_rpc* method is called:

* **test()** - broker test (ok: true)
* **info()** - broker info (author and version)
* **stats()** - broker statistics
* **client.list()** - list all connected clients
* **benchmark.test(payload)** - test method, returns the payload as-is

The payload exchange format (call params / replies) is MessagePack.

Stand-alone broker server
=========================

To build a stand-alone broker server, use the command:

.. code:: shell
    
    cargo build --features server,rpc

The *rpc* feature is optional.

Embedded broker
===============

.. note::

    If compiling for "musl" target, it is strongly recommended to replace the
    default MUSL allocator with 3rd party, e.g. with `jemallocator
    <https://crates.io/crates/jemallocator>`_ to keep the broker fast.

Example of a broker with inter-thread communications and external clients:

.. literalinclude:: ../examples/inter_thread.rs
    :language: rust
