#!/bin/sh

CMD=$1

shift

case ${CMD} in
  server)
    cargo run --release --features server,rpc --bin busrtd -- -B /tmp/busrt.sock \
      -B 0.0.0.0:9924 -B fifo:/tmp/busrt.fifo $*
    ;;
  cli)
    cargo run --release --bin busrt --features cli -- /tmp/busrt.sock $*
    ;;
  *)
    echo "command unknown: ${CMD}"
    ;;
esac
