use std::io::{self, Read, Write};

extern crate http_server;

use http_server::common::Stream;

pub struct MockTcpStream {
    read_buffer: Vec<u8>,
    write_buffer: Vec<u8>,
}

impl MockTcpStream {
    pub fn from(read_buffer: &str) -> Self {
        MockTcpStream {
            read_buffer: read_buffer.as_bytes().to_vec(),
            write_buffer: Vec::new(),
        }
    }
}

impl Read for MockTcpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let size: usize = buf.len().min(self.read_buffer.len());
        buf[..size].copy_from_slice(&&self.read_buffer[..size]);
        Ok(size)
    }
}

impl Write for MockTcpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write_buffer = Vec::from(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Stream for MockTcpStream {}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_empty_request() {
        let get_request = "GET / HTTP/1.1\r\n\r\n";
        let mut stream = MockTcpStream::from(&get_request);

        let response = http_server::handel_connection(&mut stream);
        let expected_response = format!("HTTP/1.1 200 OK\r\n\r\n");

        assert!(response.is_ok());
        assert!(stream
            .write_buffer
            .starts_with(expected_response.as_bytes()));
    }
}
