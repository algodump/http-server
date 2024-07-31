use std::net::TcpListener;
use threadpool::ThreadPool;

pub mod common;
mod request;
mod response;

fn main() {
    let listener = TcpListener::bind("127.0.0.1:4221").unwrap();
    let pool = ThreadPool::new(4);

    for stream in listener.incoming() {
        let mut stream = stream.unwrap();
        pool.execute(move || match http_server::handel_connection(&mut stream) {
            Err(err) => println!("{:?}", err),
            _ => (),
        });
    }
}
