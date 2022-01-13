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

* **list_clients()** - list all connected clients

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

Example of a broker with inter-thread communications and external clients:

.. literalinclude:: ../examples/inter_thread.rs
    :language: rust
