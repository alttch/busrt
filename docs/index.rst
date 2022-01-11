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

.. include:: readme.rst

.. toctree::
    :caption: Technical documentation
    :maxdepth: 1

    Protocol specification <protocol>
    RPC layer specification <rpc-protocol>

.. toctree::
    :caption: Bindings
    :maxdepth: 1

    Rust client/broker <https://docs.rs/elbus>
    Python client (sync) <python/elbus>
    Python client (async) <python/elbus_async>
    Javascript client <https://www.npmjs.com/package/elbus>
