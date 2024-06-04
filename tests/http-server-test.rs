extern crate http_server;

use std::io::{self, Read, Write};

pub struct MockTcpStream {
    read_buffer: Vec<u8>,
    write_buffer: Vec<u8>,
}

impl MockTcpStream {
    pub fn new() -> Self {
        MockTcpStream {
            read_buffer: Vec::new(),
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

impl http_server::Stream for MockTcpStream {}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_request() {
    }
}