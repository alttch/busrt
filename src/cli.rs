use async_trait::async_trait;
use atty::Stream;
use clap::{Parser, Subcommand};
use colored::Colorize;
use elbus::client::AsyncClient;
use elbus::common::ClientList;
use elbus::ipc::{Client, Config};
use elbus::rpc::{DummyHandlers, Rpc, RpcClient, RpcEvent, RpcHandlers, RpcResult};
use elbus::{empty_payload, Error, Frame, QoS};
use log::info;
use num_format::{Locale, ToFormattedString};
use serde_value::Value;
use std::collections::BTreeMap;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::time::sleep;

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
    //#[clap(help = "List registered clients")]
    Clients,
}

#[derive(Parser, Clone)]
struct ListenCommand {
    #[clap(short = 't', long = "topics", help = "Subscribe to topics")]
    topics: Vec<String>,
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
    #[clap(help = "payload string key=value, empty for stdin payload or '_' for no params")]
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

#[derive(Clone, Subcommand)]
enum Command {
    #[clap(subcommand)]
    Broker(BrokerCommand),
    Listen(ListenCommand),
    r#Send(TargetPayload),
    Publish(PublishCommand),
    #[clap(subcommand)]
    Rpc(RpcCommand),
}

#[derive(Parser)]
#[clap(version = elbus::VERSION, author = elbus::AUTHOR)]
struct Opts {
    #[clap(name = "socket path or host:port")]
    path: String,
    #[clap(short = 'n', long = "--name")]
    name: Option<String>,
    #[clap(short = 'v', long = "--verbose")]
    verbose: bool,
    #[clap(short = 's', long = "--silent", help = "suppress logging")]
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
fn decode_msgpack(payload: &[u8]) -> Result<BTreeMap<Value, Value>, rmp_serde::decode::Error> {
    rmp_serde::from_read_ref(payload)
}

#[inline]
fn decode_json(payload: &str) -> Result<BTreeMap<Value, Value>, serde_json::Error> {
    serde_json::from_str(payload)
}

fn print_payload(payload: &[u8]) {
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
                println!("JSON:");
                println!("{}", serde_json::to_string_pretty(&j).unwrap());
            } else {
                println!("STR: '{}'", s);
            }
            return;
        }
    }
    if let Ok(map) = decode_msgpack(payload) {
        println!("MSGPACK:");
        if let Ok(s) = serde_json::to_string_pretty(&map) {
            println!("{}", s);
        } else {
            for (k, v) in map {
                println!("{:?}: {}", k, v.to_debug_string().blue());
            }
        }
    } else {
        let (p, dots) = if payload.len() > 256 {
            (&payload[..256], "...")
        } else {
            #[allow(clippy::redundant_slicing)]
            (&payload[..], "")
        };
        println!("HEX: {}{}", hex::encode(p), dots);
    }
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
        .for_each(|t| info!("subscribing to the topic {}", t.yellow()));
    client
        .subscribe_bulk(topics.iter().map(String::as_str).collect(), QoS::Processed)
        .await
        .unwrap()
        .unwrap()
        .await
        .unwrap()
}

fn print_frame(frame: &Frame) {
    info!("Incoming frame {} byte(s)", fnum!(frame.payload().len()));
    println!(
        "{} from {}",
        frame.kind().to_debug_string().yellow(),
        frame.sender().bold()
    );
    if let Some(topic) = frame.topic() {
        println!("topic: {}", topic.magenta());
    }
    print_payload(frame.payload());
    sep();
}

struct Handlers {}

#[async_trait]
impl RpcHandlers for Handlers {
    async fn handle_frame(&self, frame: Frame) {
        print_frame(&frame);
    }
    async fn handle_notification(&self, event: RpcEvent) {
        info!(
            "Incoming RPC notification {} byte(s)",
            fnum!(event.payload().len())
        );
        println!(
            "{} from {}",
            event.kind().to_debug_string().yellow(),
            event.sender().bold()
        );
        print_payload(event.payload());
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
        println!("from {}", event.sender().bold());
        print_payload(event.payload());
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

#[allow(clippy::too_many_lines)]
#[tokio::main]
async fn main() {
    let opts = Opts::parse();
    let name = opts.name.unwrap_or_else(|| {
        format!(
            "cli.{}.{}",
            hostname::get()
                .expect("Unable to get hostname")
                .to_str()
                .expect("Unable to parse hostname"),
            std::process::id()
        )
    });
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
    info!("Connecting to {}, using service name {}", opts.path, name);
    let config = Config::new(&opts.path, &name);
    let mut client = Client::connect(&config)
        .await
        .expect("Unable to connect to the elbus broker");
    macro_rules! prepare_rpc_call {
        ($c: expr, $client: expr) => {{
            let rpc = RpcClient::new($client, DummyHandlers {});
            let payload = if $c.params.is_empty() {
                read_stdin().await
            } else {
                if $c.params.len() == 1 && $c.params[0] == "_" {
                    Vec::new()
                } else {
                    let s = $c.params.iter().map(|s| s.as_str()).collect::<Vec<&str>>();
                    rmp_serde::to_vec_named(&elbus::common::str_to_params_map(&s).unwrap()).unwrap()
                }
            };
            (rpc, payload)
        }};
    }
    match opts.command {
        Command::Broker(op) => match op {
            BrokerCommand::Clients => {
                let mut rpc = RpcClient::new(client, DummyHandlers {});
                let result = rpc
                    .call(".broker", "list_clients", empty_payload!())
                    .await
                    .unwrap();
                let mut clients: ClientList = rmp_serde::from_read_ref(result.payload()).unwrap();
                clients.clients.sort();
                let mut table = ctable(vec!["name", "type", "source", "port"]);
                for c in clients.clients {
                    if c.name != name {
                        table.add_row(row![
                            c.name,
                            c.tp,
                            c.source.unwrap_or_default(),
                            c.port.unwrap_or_default()
                        ]);
                    }
                }
                table.printstd();
            }
        },
        Command::Listen(cmd) => {
            subscribe_topics(&mut client, &cmd.topics).await.unwrap();
            sep();
            let rx = client.take_event_channel().unwrap();
            println!("Listening to messages for {} ...", name.cyan().bold());
            while let Ok(frame) = rx.recv().await {
                print_frame(&frame);
            }
        }
        Command::r#Send(cmd) => {
            let payload = get_payload(&cmd.payload).await;
            let fut = if cmd.target.contains(&['*', '?'][..]) {
                client.send_broadcast(&cmd.target, payload.into(), QoS::Processed)
            } else {
                client.send(&cmd.target, payload.into(), QoS::Processed)
            };
            fut.await.unwrap().unwrap().await.unwrap().unwrap();
            ok!();
        }
        Command::Publish(cmd) => {
            let payload = get_payload(&cmd.payload).await;
            client
                .publish(&cmd.topic, payload.into(), QoS::Processed)
                .await
                .unwrap()
                .unwrap()
                .await
                .unwrap()
                .unwrap();
            ok!();
        }
        Command::Rpc(r) => match r {
            RpcCommand::Listen(cmd) => {
                subscribe_topics(&mut client, &cmd.topics).await.unwrap();
                let rpc = RpcClient::new(client, Handlers {});
                sep();
                println!("Listening to RPC messages for {} ...", name.cyan().bold());
                let sleep_step = Duration::from_millis(100);
                while rpc.is_connected() {
                    sleep(sleep_step).await;
                }
            }
            RpcCommand::Notify(cmd) => {
                let mut rpc = RpcClient::new(client, DummyHandlers {});
                let payload = get_payload(&cmd.payload).await;
                rpc.notify(&cmd.target, payload.into(), QoS::Processed)
                    .await
                    .unwrap()
                    .unwrap()
                    .await
                    .unwrap()
                    .unwrap();
                ok!();
            }
            RpcCommand::Call0(cmd) => {
                let (rpc, payload) = prepare_rpc_call!(cmd, client);
                rpc.call0(&cmd.target, &cmd.method, payload.into())
                    .await
                    .unwrap()
                    .unwrap()
                    .await
                    .unwrap()
                    .unwrap();
                ok!();
            }
            RpcCommand::Call(cmd) => {
                let (mut rpc, payload) = prepare_rpc_call!(cmd, client);
                let result = rpc
                    .call(&cmd.target, &cmd.method, payload.into())
                    .await
                    .unwrap();
                print_payload(result.payload());
            }
        },
    }
}
