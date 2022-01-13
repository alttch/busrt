use clap::Clap;
use colored::Colorize;
use elbus::client::AsyncClient;
use elbus::ipc::{Client, Config};
use elbus::rpc::{DummyHandlers, Rpc, RpcClient};
use elbus::{empty_payload, Error, QoS};
use elbus::common::ClientList;
use log::info;
use num_format::{Locale, ToFormattedString};
use serde_value::Value;
use std::collections::BTreeMap;
use tokio::io::AsyncReadExt;

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

#[derive(Clap)]
enum BrokerCommand {
    #[clap(about = "List registered clients")]
    Clients,
}

#[derive(Clap)]
struct ListenCommand {
    #[clap(short = 't', long = "topics", about = "Subscribe to topics")]
    topics: Vec<String>,
}

#[derive(Clap)]
struct SendCommand {
    #[clap()]
    target: String,
    #[clap(about = "payload string or empty for stdin")]
    payload: Option<String>,
}

#[derive(Clap)]
struct PublishCommand {
    #[clap()]
    topic: String,
    #[clap(about = "payload string or empty for stdin")]
    payload: Option<String>,
}

#[derive(Clap)]
enum Command {
    Broker(BrokerCommand),
    Listen(ListenCommand),
    r#Send(SendCommand),
    Publish(PublishCommand),
    //Rpc(RpcCommand),
}

#[derive(Clap)]
#[clap(version = elbus::VERSION, author = elbus::AUTHOR)]
struct Opts {
    #[clap(name = "socket path or host:port")]
    path: String,
    #[clap(short = 'n', long = "--name")]
    name: Option<String>,
    #[clap(short = 'v', long = "--verbose")]
    verbose: bool,
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
    env_logger::Builder::new()
        .target(env_logger::Target::Stdout)
        .filter_level(if opts.verbose {
            log::LevelFilter::Trace
        } else {
            log::LevelFilter::Info
        })
        .init();
    info!("Connecting to {}, using service name {}", opts.path, name);
    let config = Config::new(&opts.path, &name);
    let mut client = Client::connect(&config)
        .await
        .expect("Unable to connect to elbus");
    macro_rules! get_payload {
        ($p: expr) => {
            if let Some(p) = $p {
                p.as_bytes().to_vec()
            } else {
                let mut stdin = tokio::io::stdin();
                let mut buf: Vec<u8> = Vec::new();
                stdin.read_to_end(&mut buf).await.unwrap();
                buf
            }
        };
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
            let rx = client.take_event_channel().unwrap();
            cmd.topics
                .iter()
                .for_each(|t| info!("subscribing to the topic {}", t.yellow()));
            client
                .subscribe_bulk(
                    cmd.topics.iter().map(String::as_str).collect(),
                    QoS::Processed,
                )
                .await
                .unwrap()
                .unwrap()
                .await
                .unwrap()
                .unwrap();
            sep();
            println!("Listening to messages for {} ...", name.cyan().bold());
            while let Ok(frame) = rx.recv().await {
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
        }
        Command::r#Send(cmd) => {
            let payload = get_payload!(cmd.payload);
            let fut = if cmd.target.contains(&['*', '?'][..]) {
                client.send_broadcast(&cmd.target, payload.into(), QoS::Processed)
            } else {
                client.send(&cmd.target, payload.into(), QoS::Processed)
            };
            fut.await.unwrap().unwrap().await.unwrap().unwrap();
            ok!();
        }
        Command::Publish(cmd) => {
            let payload = get_payload!(cmd.payload);
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
    }
}
