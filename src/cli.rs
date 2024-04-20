use async_trait::async_trait;
use atty::Stream;
use busrt::client::AsyncClient;
use busrt::common::{BrokerInfo, BrokerStats, ClientList};
use busrt::ipc::{Client, Config};
use busrt::rpc::{DummyHandlers, Rpc, RpcClient, RpcError, RpcEvent, RpcHandlers, RpcResult};
use busrt::{empty_payload, Error, Frame, QoS};
use clap::{Parser, Subcommand};
use colored::Colorize;
use log::{error, info};
use num_format::{Locale, ToFormattedString};
use serde_value::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio::time::sleep;

#[macro_use]
extern crate bma_benchmark;

#[cfg(not(feature = "std-alloc"))]
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;

#[macro_use]
extern crate prettytable;

trait ToDebugString<T> {
    fn to_debug_string(&self) -> String;
}

impl<T> ToDebugString<T> for T
where
    T: std::fmt::Debug,
{
    #[inline]
    fn to_debug_string(&self) -> String {
        format!("{:?}", self)
    }
}

#[derive(Subcommand, Clone)]
enum BrokerCommand {
    #[clap(name = "client.list")]
    ClientList,
    #[clap(name = "info")]
    Info,
    #[clap(name = "stats")]
    Stats,
    #[clap(name = "test")]
    Test,
}

#[derive(Parser, Clone)]
struct ListenCommand {
    #[clap(short = 't', long = "topics", help = "Subscribe to topics")]
    topics: Vec<String>,
    #[clap(long = "exclude", help = "Exclude topics")]
    exclude_topics: Vec<String>,
}

#[derive(Parser, Clone)]
struct TargetPayload {
    #[clap()]
    target: String,
    #[clap(help = "payload string or empty for stdin")]
    payload: Option<String>,
}

#[derive(Parser, Clone)]
struct RpcCall {
    #[clap()]
    target: String,
    #[clap()]
    method: String,
    #[clap(help = "payload string key=value, '-' for stdin payload")]
    params: Vec<String>,
}

#[derive(Parser, Clone)]
struct PublishCommand {
    #[clap()]
    topic: String,
    #[clap(help = "payload string or empty for stdin")]
    payload: Option<String>,
}

#[derive(Subcommand, Clone)]
enum RpcCommand {
    Listen(RpcListenCommand),
    Notify(TargetPayload),
    Call0(RpcCall),
    Call(RpcCall),
}

#[derive(Parser, Clone)]
struct RpcListenCommand {
    #[clap(short = 't', long = "topics", help = "Subscribe to topics")]
    topics: Vec<String>,
}

#[derive(Parser, Clone)]
struct BenchmarkCommand {
    #[clap(short = 'w', long = "workers", default_value = "1")]
    workers: u32,
    #[clap(long = "payload-size", default_value = "100")]
    payload_size: usize,
    #[clap(short = 'i', long = "iters", default_value = "1000000")]
    iters: u32,
}

#[derive(Clone, Subcommand)]
enum Command {
    #[clap(subcommand)]
    Broker(BrokerCommand),
    Listen(ListenCommand),
    r#Send(TargetPayload),
    Publish(PublishCommand),
    #[clap(subcommand)]
    Rpc(RpcCommand),
    Benchmark(BenchmarkCommand),
}

#[derive(Parser)]
#[clap(version = busrt::VERSION, author = busrt::AUTHOR)]
struct Opts {
    #[clap(name = "socket path or host:port")]
    path: String,
    #[clap(short = 'n', long = "name")]
    name: Option<String>,
    #[clap(long = "buf-size", default_value = "8192")]
    buf_size: usize,
    #[clap(long = "queue-size", default_value = "8192")]
    queue_size: usize,
    #[clap(long = "timeout", default_value = "5")]
    timeout: f32,
    #[clap(short = 'v', long = "verbose")]
    verbose: bool,
    #[clap(short = 's', long = "silent", help = "suppress logging")]
    silent: bool,
    #[clap(subcommand)]
    command: Command,
}

fn ctable(titles: Vec<&str>) -> prettytable::Table {
    let mut table = prettytable::Table::new();
    let format = prettytable::format::FormatBuilder::new()
        .column_separator(' ')
        .borders(' ')
        .separators(
            &[prettytable::format::LinePosition::Title],
            prettytable::format::LineSeparator::new('-', '-', '-', '-'),
        )
        .padding(0, 1)
        .build();
    table.set_format(format);
    let mut titlevec: Vec<prettytable::Cell> = Vec::new();
    for t in titles {
        titlevec.push(prettytable::Cell::new(t).style_spec("Fb"));
    }
    table.set_titles(prettytable::Row::new(titlevec));
    table
}

#[inline]
fn decode_msgpack(payload: &[u8]) -> Result<Value, rmp_serde::decode::Error> {
    rmp_serde::from_slice(payload)
}

#[inline]
fn decode_json(payload: &str) -> Result<BTreeMap<Value, Value>, serde_json::Error> {
    serde_json::from_str(payload)
}

async fn print_payload(payload: &[u8], silent: bool) {
    let mut isstr = true;
    for p in payload {
        if *p < 9 {
            isstr = false;
            break;
        }
    }
    if isstr {
        if let Ok(s) = std::str::from_utf8(payload) {
            if let Ok(j) = decode_json(s) {
                if !silent {
                    println!("JSON:");
                }
                println!(
                    "{}",
                    if silent {
                        serde_json::to_string(&j)
                    } else {
                        serde_json::to_string_pretty(&j)
                    }
                    .unwrap()
                );
            } else {
                println!("{} {}", if silent { "" } else { "STR: " }, s);
            }
            return;
        }
    }
    if let Ok(data) = decode_msgpack(payload) {
        if !silent {
            println!("MSGPACK:");
        }
        if let Ok(s) = if silent {
            serde_json::to_string(&data)
        } else {
            serde_json::to_string_pretty(&data)
        } {
            println!("{}", s);
        } else {
            print_hex(payload);
        }
    } else if silent {
        let mut stdout = tokio::io::stdout();
        stdout.write_all(payload).await.unwrap();
    } else {
        print_hex(payload);
    }
}

fn print_hex(payload: &[u8]) {
    let (p, dots) = if payload.len() > 256 {
        (&payload[..256], "...")
    } else {
        #[allow(clippy::redundant_slicing)]
        (&payload[..], "")
    };
    println!("HEX: {}{}", hex::encode(p), dots);
}

#[inline]
fn sep() {
    println!("{}", "----".dimmed());
}

macro_rules! fnum {
    ($n: expr) => {
        $n.to_formatted_string(&Locale::en).replace(',', "_")
    };
}

macro_rules! ok {
    () => {
        println!("{}", "OK".green());
    };
}

#[allow(clippy::needless_for_each)]
async fn subscribe_topics(client: &mut Client, topics: &[String]) -> Result<(), Error> {
    topics
        .iter()
        .for_each(|t| info!("subscribing to a topic {}", t.yellow()));
    client
        .subscribe_bulk(
            &topics.iter().map(String::as_str).collect::<Vec<&str>>(),
            QoS::Processed,
        )
        .await
        .unwrap()
        .unwrap()
        .await
        .unwrap()
}

#[allow(clippy::needless_for_each)]
async fn exclude_topics(client: &mut Client, topics: &[String]) -> Result<(), Error> {
    topics
        .iter()
        .for_each(|t| info!("excluding a topic {}", t.yellow()));
    client
        .exclude_bulk(
            &topics.iter().map(String::as_str).collect::<Vec<&str>>(),
            QoS::Processed,
        )
        .await
        .unwrap()
        .unwrap()
        .await
        .unwrap()
}

async fn print_frame(frame: &Frame) {
    info!("Incoming frame {} byte(s)", fnum!(frame.payload().len()));
    println!(
        "{} from {} ({})",
        frame.kind().to_debug_string().yellow(),
        frame.sender().bold(),
        frame.primary_sender()
    );
    if let Some(topic) = frame.topic() {
        println!("topic: {}", topic.magenta());
    }
    print_payload(frame.payload(), false).await;
    sep();
}

struct Handlers {}

#[async_trait]
impl RpcHandlers for Handlers {
    async fn handle_frame(&self, frame: Frame) {
        print_frame(&frame).await;
    }
    async fn handle_notification(&self, event: RpcEvent) {
        info!(
            "Incoming RPC notification {} byte(s)",
            fnum!(event.payload().len())
        );
        println!(
            "{} from {} ({})",
            event.kind().to_debug_string().yellow(),
            event.sender().bold(),
            event.primary_sender()
        );
        print_payload(event.payload(), false).await;
        sep();
    }
    async fn handle_call(&self, event: RpcEvent) -> RpcResult {
        info!("Incoming RPC call");
        println!(
            "method: {}",
            event
                .parse_method()
                .map_or_else(
                    |_| format!("HEX: {}", hex::encode(event.method())),
                    ToOwned::to_owned
                )
                .blue()
                .bold()
        );
        println!(
            "from {} ({})",
            event.sender().bold(),
            event.primary_sender()
        );
        print_payload(event.payload(), false).await;
        sep();
        Ok(None)
    }
}

async fn read_stdin() -> Vec<u8> {
    let mut stdin = tokio::io::stdin();
    let mut buf: Vec<u8> = Vec::new();
    if atty::is(Stream::Stdin) {
        println!("Reading stdin, Ctrl-D to finish...");
    }
    stdin.read_to_end(&mut buf).await.unwrap();
    buf
}

async fn get_payload(candidate: &Option<String>) -> Vec<u8> {
    if let Some(p) = candidate {
        p.as_bytes().to_vec()
    } else {
        read_stdin().await
    }
}

async fn create_client(opts: &Opts, name: &str) -> Client {
    let config = Config::new(&opts.path, name)
        .buf_size(opts.buf_size)
        .queue_size(opts.queue_size)
        .timeout(Duration::from_secs_f32(opts.timeout));
    Client::connect(&config)
        .await
        .expect("Unable to connect to the busrt broker")
}

macro_rules! bm_finish {
    ($iters: expr, $futs: expr) => {
        while let Some(f) = $futs.pop() {
            f.await.unwrap();
        }
        staged_benchmark_finish_current!($iters);
    };
}

async fn benchmark_client(
    opts: &Opts,
    client_name: &str,
    iters: u32,
    workers: u32,
    payload_size: usize,
) {
    let iters_worker = iters / workers;
    let data = Arc::new(vec![0xee; payload_size]);
    let mut clients = Vec::new();
    let mut ecs = Vec::new();
    let mut cnns = Vec::new();
    let mut futs = Vec::new();
    for w in 0..workers {
        let cname = format!("{}-{}", client_name, w + 1);
        let cname_null = format!("{}-{}-null", client_name, w + 1);
        let mut client = create_client(opts, &cname).await;
        let rx = client.take_event_channel().unwrap();
        clients.push(Arc::new(Mutex::new(client)));
        ecs.push(Arc::new(Mutex::new(rx)));
        cnns.push(cname_null);
    }
    macro_rules! clear {
        () => {
            for e in &ecs {
                let rx = e.lock().await;
                while !rx.is_empty() {
                    let _r = rx.recv().await;
                }
            }
        };
    }
    macro_rules! spawn_sender {
        ($client: expr, $target: expr, $payload: expr, $qos: expr) => {
            futs.push(tokio::spawn(async move {
                let mut client = $client.lock().await;
                for _ in 0..iters_worker {
                    let result = client
                        .send(&$target, $payload.clone().into(), $qos)
                        .await
                        .unwrap();
                    if $qos.needs_ack() {
                        let _r = result.unwrap().await.unwrap();
                    }
                }
            }));
        };
    }
    for q in &[(QoS::No, "no"), (QoS::RealtimeProcessed, "processed")] {
        let qos = q.0;
        clear!();
        staged_benchmark_start!(&format!("send.qos.{}", q.1));
        for w in 0..workers {
            let client = clients[w as usize].clone();
            let payload = data.clone();
            let target = cnns[w as usize].clone();
            spawn_sender!(client, target, payload, qos);
        }
        bm_finish!(iters, futs);
    }
    for q in &[(QoS::No, "no"), (QoS::RealtimeProcessed, "processed")] {
        let qos = q.0;
        clear!();
        staged_benchmark_start!(&format!("send+recv.qos.{}", q.1));
        for w in 0..workers {
            let client = clients[w as usize].clone();
            let target = client.lock().await.get_name().to_owned();
            let payload = data.clone();
            spawn_sender!(client, target, payload, qos);
            let crx = ecs[w as usize].clone();
            futs.push(tokio::spawn(async move {
                let rx = crx.lock().await;
                let mut cnt = 0;
                while cnt < iters_worker {
                    let _r = rx.recv().await;
                    cnt += 1;
                }
            }));
        }
        bm_finish!(iters, futs);
    }
}

struct BenchmarkHandlers {}

#[async_trait]
impl RpcHandlers for BenchmarkHandlers {
    async fn handle_frame(&self, _frame: Frame) {}
    async fn handle_notification(&self, _event: RpcEvent) {}
    async fn handle_call(&self, event: RpcEvent) -> RpcResult {
        if event.parse_method()? == "benchmark.selftest" {
            Ok(Some(event.payload().to_vec()))
        } else {
            Err(RpcError::method(None))
        }
    }
}

async fn benchmark_rpc(
    opts: &Opts,
    client_name: &str,
    iters: u32,
    workers: u32,
    payload_size: usize,
) {
    let iters_worker = iters / workers;
    let data = Arc::new(vec![0xee; payload_size]);
    let mut rpcs = Vec::new();
    let mut cnns = Vec::new();
    let mut futs = Vec::new();
    for w in 0..workers {
        let cname = format!("{}-{}", client_name, w + 1);
        let cname_null = format!("{}-{}-null", client_name, w + 1);
        let client = create_client(opts, &cname).await;
        let rpc = RpcClient::new(client, BenchmarkHandlers {});
        rpcs.push(Arc::new(Mutex::new(rpc)));
        cnns.push(cname_null);
    }
    macro_rules! spawn_caller {
        ($rpc: expr, $target: expr, $method: expr, $payload: expr, $cr: expr) => {
            futs.push(tokio::spawn(async move {
                let rpc = $rpc.lock().await;
                for _ in 0..iters_worker {
                    if $cr {
                        let result = rpc
                            .call(
                                &$target,
                                $method,
                                $payload.clone().into(),
                                QoS::RealtimeProcessed,
                            )
                            .await
                            .unwrap();
                        assert_eq!(result.payload(), *$payload);
                    } else {
                        let result = rpc
                            .call0(
                                &$target,
                                $method,
                                $payload.clone().into(),
                                QoS::RealtimeProcessed,
                            )
                            .await
                            .unwrap();
                        let _r = result.unwrap().await.unwrap();
                    }
                }
            }));
        };
    }
    staged_benchmark_start!("rpc.call");
    for w in 0..workers {
        let rpc = rpcs[w as usize].clone();
        let payload = data.clone();
        spawn_caller!(rpc, ".broker", "benchmark.test", payload, true);
    }
    bm_finish!(iters, futs);
    staged_benchmark_start!("rpc.call+handle");
    for w in 0..workers {
        let rpc = rpcs[w as usize].clone();
        let target = rpc.lock().await.client().lock().await.get_name().to_owned();
        let payload = data.clone();
        spawn_caller!(rpc, target, "benchmark.selftest", payload, true);
    }
    bm_finish!(iters, futs);
    staged_benchmark_start!("rpc.call0");
    for w in 0..workers {
        let rpc = rpcs[w as usize].clone();
        let target = cnns[w as usize].clone();
        let payload = data.clone();
        spawn_caller!(rpc, target, "test", payload, false);
    }
    bm_finish!(iters, futs);
}

#[allow(clippy::too_many_lines)]
#[tokio::main(worker_threads = 1)]
async fn main() {
    let opts = Opts::parse();
    let client_name = opts.name.as_ref().map_or_else(
        || {
            format!(
                "cli.{}.{}",
                hostname::get()
                    .expect("Unable to get hostname")
                    .to_str()
                    .expect("Unable to parse hostname"),
                std::process::id()
            )
        },
        ToOwned::to_owned,
    );
    if !opts.silent {
        env_logger::Builder::new()
            .target(env_logger::Target::Stdout)
            .filter_level(if opts.verbose {
                log::LevelFilter::Trace
            } else {
                log::LevelFilter::Info
            })
            .init();
    }
    info!(
        "Connecting to {}, using service name {}",
        opts.path, client_name
    );
    macro_rules! prepare_rpc_call {
        ($c: expr, $client: expr) => {{
            let rpc = RpcClient::new($client, DummyHandlers {});
            let payload = if $c.params.len() == 1 && $c.params[0] == "-" {
                read_stdin().await
            } else if $c.params.is_empty() {
                Vec::new()
            } else {
                let s = $c.params.iter().map(String::as_str).collect::<Vec<&str>>();
                rmp_serde::to_vec_named(&busrt::common::str_to_params_map(&s).unwrap()).unwrap()
            };
            (rpc, payload)
        }};
    }
    let timeout = Duration::from_secs_f32(opts.timeout);
    macro_rules! wto {
        ($fut: expr) => {
            tokio::time::timeout(timeout, $fut)
                .await
                .expect("timed out")
        };
    }
    match opts.command {
        Command::Broker(ref op) => {
            let client = create_client(&opts, &client_name).await;
            match op {
                BrokerCommand::ClientList => {
                    let rpc = RpcClient::new(client, DummyHandlers {});
                    let result =
                        wto!(rpc.call(".broker", "client.list", empty_payload!(), QoS::Processed))
                            .unwrap();
                    let mut clients: ClientList = rmp_serde::from_slice(result.payload()).unwrap();
                    clients.clients.sort();
                    let mut table = ctable(vec![
                        "name", "type", "source", "port", "r_frames", "r_bytes", "w_frames",
                        "w_bytes", "queue", "ins",
                    ]);
                    for c in clients.clients {
                        if c.name != client_name {
                            table.add_row(row![
                                c.name,
                                c.kind,
                                c.source.unwrap_or_default(),
                                c.port.unwrap_or_default(),
                                fnum!(c.r_frames),
                                fnum!(c.r_bytes),
                                fnum!(c.w_frames),
                                fnum!(c.w_bytes),
                                fnum!(c.queue),
                                fnum!(c.instances),
                            ]);
                        }
                    }
                    table.printstd();
                }
                BrokerCommand::Stats => {
                    let rpc = RpcClient::new(client, DummyHandlers {});
                    let result =
                        wto!(rpc.call(".broker", "stats", empty_payload!(), QoS::Processed))
                            .unwrap();
                    let stats: BrokerStats = rmp_serde::from_slice(result.payload()).unwrap();
                    let mut table = ctable(vec!["field", "value"]);
                    table.add_row(row!["r_frames", stats.r_frames]);
                    table.add_row(row!["r_bytes", stats.r_bytes]);
                    table.add_row(row!["w_frames", stats.w_frames]);
                    table.add_row(row!["w_bytes", stats.w_bytes]);
                    table.add_row(row!["uptime", stats.uptime]);
                    table.printstd();
                }
                BrokerCommand::Info => {
                    let rpc = RpcClient::new(client, DummyHandlers {});
                    let result =
                        wto!(rpc.call(".broker", "info", empty_payload!(), QoS::Processed))
                            .unwrap();
                    let info: BrokerInfo = rmp_serde::from_slice(result.payload()).unwrap();
                    let mut table = ctable(vec!["field", "value"]);
                    table.add_row(row!["author", info.author]);
                    table.add_row(row!["version", info.version]);
                    table.printstd();
                }
                BrokerCommand::Test => {
                    let rpc = RpcClient::new(client, DummyHandlers {});
                    let result =
                        wto!(rpc.call(".broker", "test", empty_payload!(), QoS::Processed))
                            .unwrap();
                    print_payload(result.payload(), opts.silent).await;
                }
            }
        }
        Command::Listen(ref cmd) => {
            let mut client = create_client(&opts, &client_name).await;
            exclude_topics(&mut client, &cmd.exclude_topics)
                .await
                .unwrap();
            subscribe_topics(&mut client, &cmd.topics).await.unwrap();
            sep();
            let rx = client.take_event_channel().unwrap();
            println!(
                "Listening to messages for {} ...",
                client_name.cyan().bold()
            );
            let fut = tokio::spawn(async move {
                while let Ok(frame) = rx.recv().await {
                    print_frame(&frame).await;
                }
            });
            let sleep_step = Duration::from_millis(100);
            while client.is_connected() {
                sleep(sleep_step).await;
            }
            fut.abort();
        }
        Command::r#Send(ref cmd) => {
            let mut client = create_client(&opts, &client_name).await;
            let payload = get_payload(&cmd.payload).await;
            let fut = if cmd.target.contains(&['*', '?'][..]) {
                client.send_broadcast(&cmd.target, payload.into(), QoS::Processed)
            } else {
                client.send(&cmd.target, payload.into(), QoS::Processed)
            };
            wto!(wto!(fut).unwrap().unwrap()).unwrap().unwrap();
            ok!();
        }
        Command::Publish(ref cmd) => {
            let mut client = create_client(&opts, &client_name).await;
            let payload = get_payload(&cmd.payload).await;
            wto!(
                wto!(client.publish(&cmd.topic, payload.into(), QoS::Processed))
                    .unwrap()
                    .unwrap()
            )
            .unwrap()
            .unwrap();
            ok!();
        }
        Command::Rpc(ref r) => {
            let mut client = create_client(&opts, &client_name).await;
            match r {
                RpcCommand::Listen(cmd) => {
                    subscribe_topics(&mut client, &cmd.topics).await.unwrap();
                    let rpc = RpcClient::new(client, Handlers {});
                    sep();
                    println!(
                        "Listening to RPC messages for {} ...",
                        client_name.cyan().bold()
                    );
                    let sleep_step = Duration::from_millis(100);
                    while rpc.is_connected() {
                        sleep(sleep_step).await;
                    }
                }
                RpcCommand::Notify(cmd) => {
                    let rpc = RpcClient::new(client, DummyHandlers {});
                    let payload = get_payload(&cmd.payload).await;
                    wto!(
                        wto!(rpc.notify(&cmd.target, payload.into(), QoS::Processed))
                            .unwrap()
                            .unwrap()
                    )
                    .unwrap()
                    .unwrap();
                    ok!();
                }
                RpcCommand::Call0(cmd) => {
                    let (rpc, payload) = prepare_rpc_call!(cmd, client);
                    wto!(
                        wto!(rpc.call0(&cmd.target, &cmd.method, payload.into(), QoS::Processed))
                            .unwrap()
                            .unwrap()
                    )
                    .unwrap()
                    .unwrap();
                    ok!();
                }
                RpcCommand::Call(cmd) => {
                    let (rpc, payload) = prepare_rpc_call!(cmd, client);
                    match wto!(rpc.call(&cmd.target, &cmd.method, payload.into(), QoS::Processed)) {
                        Ok(result) => print_payload(result.payload(), opts.silent).await,
                        Err(e) => {
                            let message = e
                                .data()
                                .map_or("", |data| std::str::from_utf8(data).unwrap_or(""));
                            error!("RPC Error {}: {}", e.code(), message);
                            std::process::exit(1);
                        }
                    }
                }
            }
        }
        Command::Benchmark(ref cmd) => {
            println!(
                "Starting benchmark, {} worker(s), {} iters, {} iters/worker, {} byte(s) payload",
                cmd.workers.to_string().blue().bold(),
                fnum!(cmd.iters).yellow(),
                fnum!(cmd.iters / cmd.workers).bold(),
                fnum!(cmd.payload_size).cyan()
            );
            benchmark_client(
                &opts,
                &client_name,
                cmd.iters,
                cmd.workers,
                cmd.payload_size,
            )
            .await;
            benchmark_rpc(
                &opts,
                &client_name,
                cmd.iters,
                cmd.workers,
                cmd.payload_size,
            )
            .await;
            staged_benchmark_print!();
        }
    }
}
