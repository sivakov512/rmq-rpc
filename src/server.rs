use lapin::{Channel, Connection, ConnectionProperties};
use tokio_amqp::*;

#[allow(dead_code)]
pub struct RmqRpcServer {
    conn: Connection,
    channel: Channel,
}

impl RmqRpcServer {
    pub async fn connect(url: &str) -> Result<Self, lapin::Error> {
        let conn = Connection::connect(url, ConnectionProperties::default().with_tokio()).await?;
        let channel = conn.create_channel().await?;

        Ok(Self { conn, channel })
    }
}

#[cfg(test)]
mod connect {
    use super::*;

    #[tokio::test]
    async fn returns_server_if_connected_successfully() {
        let got = RmqRpcServer::connect("amqp://127.0.0.1:5672").await;

        assert!(got.is_ok())
    }

    #[tokio::test]
    async fn errored_if_something_went_wrong() {
        let got = RmqRpcServer::connect("amqp://127.0.0.1:5673").await;

        assert!(got.is_err())
    }
}
