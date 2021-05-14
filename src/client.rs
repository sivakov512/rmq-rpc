use futures_util::stream::StreamExt;
use lapin::{
    message::Delivery,
    options::{BasicConsumeOptions, BasicPublishOptions, QueueDeclareOptions},
    types::FieldTable,
    BasicProperties, Channel, Connection, ConnectionProperties,
};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_amqp::*;

#[derive(Debug)]
pub struct RmqRpcClient {
    conn: Connection,
    channel: Channel,
    fast_reply_task: tokio::task::JoinHandle<()>,
    replies: Arc<Mutex<Vec<Delivery>>>,
}

impl RmqRpcClient {
    pub async fn connect(url: &str) -> Result<Self, lapin::Error> {
        let conn = Connection::connect(url, ConnectionProperties::default().with_tokio()).await?;
        let channel = conn.create_channel().await?;
        let replies = Arc::new(Mutex::new(Vec::new()));

        let cloned_channel = channel.clone();
        let cloned_replies = replies.clone();
        let fast_reply_task = tokio::spawn(async move {
            let mut consumer = cloned_channel
                .basic_consume(
                    "amq.rabbitmq.reply-to",
                    "",
                    BasicConsumeOptions {
                        no_ack: true,
                        ..BasicConsumeOptions::default()
                    },
                    FieldTable::default(),
                )
                .await
                .unwrap();
            while let Some(delivery) = consumer.next().await {
                let (_, delivery) = delivery.unwrap();

                cloned_replies.lock().await.push(delivery);
            }
        });

        Ok(Self {
            conn,
            channel,
            fast_reply_task,
            replies,
        })
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
                BasicProperties::default().with_reply_to("amq.rabbitmq.reply-to".into()),
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
        channel
            .queue_delete(QUEUE, lapin::options::QueueDeleteOptions::default())
            .await
            .unwrap();

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

    #[tokio::test]
    async fn message_has_reply_to_queue() {
        const QUEUE: &str = "message_has_reply_to_queue";
        const DATA: &str = "Hello";
        let channel = create_channel().await;
        channel
            .queue_delete(QUEUE, lapin::options::QueueDeleteOptions::default())
            .await
            .unwrap();

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
        assert!(msg.delivery.properties.reply_to().is_some());
        assert!(msg
            .delivery
            .properties
            .reply_to()
            .clone()
            .unwrap()
            .to_string()
            .contains("amq.rabbitmq.reply-to"));
    }

    #[tokio::test]
    async fn store_replies_inside() {
        const QUEUE: &str = "store_replies_inside";
        const PING: &str = "Ping";
        const PONG: &str = "Pong";
        let channel = create_channel().await;
        channel
            .queue_delete(QUEUE, lapin::options::QueueDeleteOptions::default())
            .await
            .unwrap();

        let client = RmqRpcClient::connect(URL).await.unwrap();
        client
            .declare_queue(QUEUE)
            .await
            .unwrap()
            .send_message(QUEUE, PING.as_bytes().to_vec())
            .await
            .unwrap();

        let msg = channel
            .basic_get(QUEUE, lapin::options::BasicGetOptions::default())
            .await
            .unwrap()
            .unwrap();
        let reply_to = msg
            .delivery
            .properties
            .reply_to()
            .clone()
            .unwrap()
            .to_string();
        channel
            .basic_publish(
                "",
                &reply_to,
                BasicPublishOptions::default(),
                PONG.as_bytes().to_vec(),
                BasicProperties::default(),
            )
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let mut replies = client.replies.lock().await;
        assert_eq!(replies.len(), 1);
        assert_eq!(String::from_utf8_lossy(&replies.pop().unwrap().data), PONG);
    }
}
