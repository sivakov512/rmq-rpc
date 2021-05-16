use crate::reply::FutureRpcReply;
use futures_util::stream::StreamExt;
use lapin::{
    message::Delivery,
    options::{BasicConsumeOptions, BasicPublishOptions, QueueDeclareOptions},
    types::FieldTable,
    BasicProperties, Channel, Connection, ConnectionProperties,
};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_amqp::*;
use uuid::Uuid;

#[derive(Debug)]
pub struct RmqRpcClient {
    conn: Connection,
    channel: Channel,
    fast_reply_task: tokio::task::JoinHandle<()>,
    replies: Arc<Mutex<BTreeMap<String, FutureRpcReply<Delivery>>>>,
}

impl RmqRpcClient {
    pub async fn connect(url: &str) -> Result<Self, lapin::Error> {
        let conn = Connection::connect(url, ConnectionProperties::default().with_tokio()).await?;
        let channel = conn.create_channel().await?;
        let replies: Arc<Mutex<BTreeMap<String, FutureRpcReply<Delivery>>>> =
            Arc::new(Mutex::new(BTreeMap::new()));

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

                match delivery.properties.correlation_id() {
                    None => unimplemented!(), // TODO: log this situation or support deliveries without replies
                    Some(v) => match cloned_replies.lock().await.remove(&v.to_string()) {
                        Some(v) => v.resolve(delivery).await,
                        None => unimplemented!(), // TODO: log this or declare future here
                    },
                };
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
    ) -> Result<Delivery, lapin::Error> {
        let correlation_id = Uuid::new_v4().to_string();

        self.channel
            .basic_publish(
                "",
                routing_key,
                BasicPublishOptions::default(),
                payload,
                BasicProperties::default()
                    .with_reply_to("amq.rabbitmq.reply-to".into())
                    .with_correlation_id(correlation_id.clone().into()),
            )
            .await?;

        let reply_fut = FutureRpcReply::new();
        self.replies
            .lock()
            .await
            .insert(correlation_id, reply_fut.clone());
        Ok(reply_fut.await)
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
    const PING: &str = "Ping";
    const PONG: &str = "Pong";

    struct TstQueue {
        name: String,
        channel: Channel,
    }

    impl TstQueue {
        async fn new(name: &str) -> Self {
            let channel = Connection::connect(URL, ConnectionProperties::default().with_tokio())
                .await
                .unwrap()
                .create_channel()
                .await
                .unwrap();

            channel
                .queue_delete(name, lapin::options::QueueDeleteOptions::default())
                .await
                .unwrap();
            channel
                .queue_declare(
                    name,
                    lapin::options::QueueDeclareOptions::default(),
                    lapin::types::FieldTable::default(),
                )
                .await
                .unwrap();

            Self {
                name: name.to_owned(),
                channel,
            }
        }

        async fn get(&self) -> lapin::message::Delivery {
            self.channel
                .basic_get(&self.name, lapin::options::BasicGetOptions::default())
                .await
                .unwrap()
                .unwrap()
                .delivery
        }

        async fn publish(
            &self,
            routing_key: &str,
            payload: Vec<u8>,
            properties: lapin::BasicProperties,
        ) {
            self.channel
                .basic_publish(
                    "",
                    routing_key,
                    BasicPublishOptions::default(),
                    payload,
                    properties,
                )
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn message_sent() {
        const QUEUE: &str = "message_sent";
        let queue = TstQueue::new(QUEUE).await;
        let client = RmqRpcClient::connect(URL).await.unwrap();

        tokio::spawn(async move {
            client
                .send_message(QUEUE, PING.as_bytes().to_vec())
                .await
                .unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;

        let delivery = queue.get().await;
        assert_eq!(String::from_utf8_lossy(&delivery.data), PING.to_owned())
    }

    #[tokio::test]
    async fn message_has_reply_to_queue() {
        const QUEUE: &str = "message_has_reply_to_queue";
        let queue = TstQueue::new(QUEUE).await;
        let client = RmqRpcClient::connect(URL).await.unwrap();

        tokio::spawn(async move {
            client
                .send_message(QUEUE, PING.as_bytes().to_vec())
                .await
                .unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;

        let delivery = queue.get().await;
        assert!(delivery.properties.reply_to().is_some());
        assert!(delivery
            .properties
            .reply_to()
            .clone()
            .unwrap()
            .to_string()
            .contains("amq.rabbitmq.reply-to"));
    }

    #[tokio::test]
    async fn message_has_correlation_id() {
        const QUEUE: &str = "message_has_correlation_id";
        let queue = TstQueue::new(QUEUE).await;
        let client = RmqRpcClient::connect(URL).await.unwrap();

        tokio::spawn(async move {
            client
                .send_message(QUEUE, PONG.as_bytes().to_vec())
                .await
                .unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;

        let delivery = queue.get().await;
        assert!(delivery.properties.correlation_id().is_some());
    }

    #[tokio::test]
    async fn returns_response() {
        const QUEUE: &str = "returns_response";
        let queue = TstQueue::new(QUEUE).await;
        let store: Arc<Mutex<Option<Delivery>>> = Arc::new(Mutex::new(None));
        let store_cloned = store.clone();

        let client = RmqRpcClient::connect(URL).await.unwrap();
        tokio::spawn(async move {
            let got = client
                .send_message(QUEUE, PING.as_bytes().to_vec())
                .await
                .unwrap();
            store_cloned.lock().await.replace(got);
        });
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;

        let delivery = queue.get().await;
        queue
            .publish(
                &delivery.properties.reply_to().clone().unwrap().to_string(),
                PONG.as_bytes().to_vec(),
                BasicProperties::default()
                    .with_correlation_id(delivery.properties.correlation_id().clone().unwrap()),
            )
            .await;
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;

        let got = store.lock().await;
        assert!(got.is_some());
        assert_eq!(
            String::from_utf8_lossy(&(got.as_ref().unwrap().data)).to_string(),
            PONG
        )
    }
}
