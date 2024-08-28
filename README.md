# Simple HTTP server 

Simple HTTP server implementation. The main purpose of building it is to learn Rust.

## Build 
```
cargo build
```

## Run 
Run with default IP address: `127.0.0.1:4421`
```
cargo run 
```
Run with custom IP address 
```
cargo run -- --ip 192.168.0.1 --port 3499
```

## TODO
- [x] Map some of the internal errors to actual HTTP response codes
- [x] Fix known security issues during the parsing 
- [x] Implement URL parsing 
- [x] Implement multipart requests
- [x] Implement simple authentication
- [x] Implement HTTP cache
- [x] Implement compression
- [ ] Implement all other methods
- [ ] Add integration test