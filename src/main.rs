use log::{error, info, LevelFilter, Metadata, Record};
use std::net::TcpListener;
use threadpool::ThreadPool;

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

pub mod common;
mod request;
mod response;

fn main() {
    log::set_logger(&CONSOLE_LOGGER).expect("Failed to set up console logger");
    log::set_max_level(LevelFilter::Info);

    let server_address = "127.0.0.1:4221";
    let listener = TcpListener::bind(server_address).unwrap();
    let pool = ThreadPool::new(4);

    info!("Server IP address: {}", server_address);

    for stream in listener.incoming() {
        let mut stream = stream.unwrap();
        pool.execute(move || match http_server::handel_connection(&mut stream) {
            Err(err) => error!("{:?}", err),
            _ => (),
        });
    }
}
