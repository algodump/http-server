
use std::net::TcpListener;
use threadpool::ThreadPool;

fn main() {
    let listener = TcpListener::bind("127.0.0.1:4221").unwrap();
    let pool = ThreadPool::new(4);

    for stream in listener.incoming() {
        let stream = stream.unwrap();
        pool.execute(|| match http_server::handel_connection(stream) {
            Err(err) => println!("{:?}", err),
            _ => (),
        });
    }
}
