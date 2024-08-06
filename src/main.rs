use clap::{arg, Parser};
use log::{error, info, LevelFilter, Metadata, Record};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
    str::FromStr,
};
use threadpool::ThreadPool;

pub mod common;
mod request;
mod response;

static CONSOLE_LOGGER: ConsoleLogger = ConsoleLogger;

struct ConsoleLogger;

impl log::Log for ConsoleLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            println!("[{}] {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

fn default_ip() -> Ipv4Addr {
    return Ipv4Addr::new(127, 0, 0, 1);
}

fn default_port() -> u16 {
    return 4421;
}

#[derive(Parser, Debug, Default)]
#[command(version, about, long_about = None)]
struct Args {
    /// Ip address of the HTTP server
    #[arg(short, long, default_value_t = default_ip().to_string())]
    ip: String,

    /// Port numbers
    #[arg(short, long, default_value_t = default_port())]
    port: u16,
}

fn main() {
    log::set_logger(&CONSOLE_LOGGER).expect("Failed to set up console logger");
    log::set_max_level(LevelFilter::Info);

    let args: Args = Args::parse();
    let ip = Ipv4Addr::from_str(&args.ip).unwrap_or_else(|_| {
        let default_ip = default_ip();
        info!(
            "Invalid IP address provided, using default: {:?}",
            default_ip
        );
        default_ip
    });

    let socket = SocketAddr::new(IpAddr::V4(ip), args.port);

    let listener = TcpListener::bind(socket).unwrap();
    let pool = ThreadPool::new(4);

    info!("Server IP address: {:?}", socket);

    for stream in listener.incoming() {
        let mut stream = stream.unwrap();
        pool.execute(move || match http_server::handel_connection(&mut stream) {
            Err(err) => error!("{:?}", err),
            _ => (),
        });
    }
}
