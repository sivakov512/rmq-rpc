use lapin::{
    options::{BasicPublishOptions, QueueDeclareOptions},
    types::FieldTable,
    BasicProperties, Channel, Connection, ConnectionProperties,
};
use tokio_amqp::*;

#[derive(Debug)]
pub struct RmqRpcClient {
    conn: Connection,
    channel: Channel,
}

impl RmqRpcClient {
    pub async fn connect(url: &str) -> Result<Self, lapin::Error> {
        let conn = Connection::connect(url, ConnectionProperties::default().with_tokio()).await?;
        let channel = conn.create_channel().await?;
        Ok(Self { conn, channel })
    }

    pub async fn declare_queue(&self, queue: &str) -> Result<&Self, lapin::Error> {
        self.channel
            .queue_declare(queue, QueueDeclareOptions::default(), FieldTable::default())
            .await?;
        Ok(self)
    }

    pub async fn send_message(
        &self,
        routing_key: &str,
        payload: Vec<u8>,
    ) -> Result<(), lapin::Error> {
        self.channel
            .basic_publish(
                "",
                routing_key,
                BasicPublishOptions::default(),
                payload,
                BasicProperties::default(),
            )
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod connect {
    use super::*;

    #[tokio::test]
    async fn returns_client_if_connected_successfully() {
        let got = RmqRpcClient::connect("amqp://127.0.0.1:5672").await;

        assert!(got.is_ok())
    }

    #[tokio::test]
    async fn errored_if_something_went_wrong() {
        let got = RmqRpcClient::connect("amqp://127.0.0.1:5673").await;

        assert!(got.is_err())
    }
}

#[cfg(test)]
mod send_message {
    use super::*;

    const URL: &str = "amqp://127.0.0.1:5672";

    async fn create_channel() -> Channel {
        Connection::connect(URL, ConnectionProperties::default().with_tokio())
            .await
            .unwrap()
            .create_channel()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn message_sent() {
        const QUEUE: &str = "message_sent";
        const DATA: &str = "Hello";
        let channel = create_channel().await;

        RmqRpcClient::connect(URL)
            .await
            .unwrap()
            .declare_queue(QUEUE)
            .await
            .unwrap()
            .send_message(QUEUE, DATA.as_bytes().to_vec())
            .await
            .unwrap();

        let msg = channel
            .basic_get(QUEUE, lapin::options::BasicGetOptions::default())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&msg.delivery.data), DATA.to_owned())
    }
}
