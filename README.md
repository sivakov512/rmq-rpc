# rmq-rpc
**Early release, not stable enough!**

[![Build status](https://github.com/sivakov512/rmq-rpc/actions/workflows/test.yml/badge.svg)](https://github.com/sivakov512/rmq-rpc/actions/workflows/test.yml)
[![Downloads](https://img.shields.io/crates/d/rmq-rpc.svg)](https://crates.io/crates/rmq-rpc)
[![API Docs](https://docs.rs/rmq-rpc/badge.svg)](https://docs.rs/rmq-rpc)
[![LICENSE](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Client\server for RPC via RabbitMQ

Please look at `examples` directory for complete examples.

## Server example

```rust
use rmq_rpc::RmqRpcServer;
use std::io::Error;

const URL: &str = "amqp://127.0.0.1:5672";
const QUEUE_NAME: &str = "examples";

async fn handler(data: Vec<u8>) -> Result<Vec<u8>, Error> {
    let mut res = data.to_owned();
    res.reverse();
    Ok(res)
}

#[tokio::main]
async fn main() {
    let server = RmqRpcServer::connect(URL).await.unwrap();
    server.declare_queue(QUEUE_NAME).await.unwrap();

    server.drain(QUEUE_NAME, handler).await.unwrap();
}
```

## Client example
```rust
use rmq_rpc::RmqRpcClient;
use std::env;

const URL: &str = "amqp://127.0.0.1:5672";
const QUEUE_NAME: &str = "examples";

#[tokio::main]
async fn main() {
    let msg = match env::args().skip(1).next().unwrap();

    let client = RmqRpcClient::connect(URL).await.unwrap();
    client.declare_queue(QUEUE_NAME).await.unwrap();

    let _ = client
        .send_message(QUEUE_NAME, msg.as_bytes().to_vec())
        .await
        .unwrap();
}
```
