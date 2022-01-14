#!/bin/sh

CMD=$1

shift

case ${CMD} in
  server)
    cargo run --release --features server,rpc --bin elbusd -- -B /tmp/elbus.sock \
      -B 0.0.0.0:9924 -B fifo:/tmp/elbus.fifo $*
    ;;
  #benchmark)
    #cargo run --release --bin psrt-cli --features cli -- localhost:2873 --benchmark $*
    #;;
  cli)
    cargo run --release --bin elbus --features cli -- /tmp/elbus.sock $*
    ;;
  *)
    echo "command unknown: ${CMD}"
    ;;
esac
