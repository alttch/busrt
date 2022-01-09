#[macro_use]
extern crate bma_benchmark;

use elbus::broker::Broker;
use elbus::client::AsyncClient;
use elbus::QoS;

#[tokio::main]
async fn main() {
    let iters: u32 = 10_000_000;
    let broker = Broker::new();
    let mut t1 = broker.register_client("t1").unwrap();
    let mut t2 = broker.register_client("t2").unwrap();
    let mut t3 = broker.register_client("t3").unwrap();
    let mut t4 = broker.register_client("t4").unwrap();
    let t2rx = t2.take_event_channel().unwrap();
    let t4rx = t4.take_event_channel().unwrap();
    let listener = tokio::spawn(async move {
        let mut c = 0;
        loop {
            let _frame = t2rx.recv().await.unwrap();
            c += 1;
            if c == iters {
                break;
            }
        }
    });
    let listener2 = tokio::spawn(async move {
        let mut c = 0;
        loop {
            let _frame = t4rx.recv().await.unwrap();
            c += 1;
            if c == iters {
                break;
            }
        }
    });
    let data = vec![0xff_u8; 100];
    let data2 = data.clone();
    benchmark_start!();
    tokio::spawn(async move {
        for _ in 0..iters {
            t1.send("t2", data.clone().into(), QoS::Processed).await.unwrap();
        }
    });
    tokio::spawn(async move {
        for _ in 0..iters {
            t3.send("t4", data2.clone().into(), QoS::Processed).await.unwrap();
        }
    });
    listener.await.unwrap();
    listener2.await.unwrap();
    benchmark_print!(iters * 2);
}
