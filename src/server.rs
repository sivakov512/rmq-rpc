use futures_util::StreamExt;
use lapin::{
    options::{BasicAckOptions, BasicConsumeOptions, BasicPublishOptions, BasicRejectOptions},
    types::FieldTable,
    BasicProperties, Channel, Connection, ConnectionProperties,
};
use std::future::Future;
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

    pub async fn drain<F, Fut, E>(&self, queue_name: &str, handler: F) -> Result<(), lapin::Error>
    where
        F: Fn(&[u8]) -> Fut,
        Fut: Future<Output = Result<Vec<u8>, E>>,
        E: std::error::Error,
    {
        let mut consumer = self
            .channel
            .basic_consume(
                queue_name,
                "",
                BasicConsumeOptions::default(),
                FieldTable::default(),
            )
            .await?;

        while let Some(delivery) = consumer.next().await {
            let (_, delivery) = delivery?;

            match handler(delivery.data.as_slice()).await {
                Ok(reply) => {
                    let reply_to = delivery.properties.reply_to().clone().unwrap();
                    let correlation_id = delivery.properties.correlation_id().clone().unwrap();
                    self.channel
                        .basic_publish(
                            "",
                            reply_to.as_str(),
                            BasicPublishOptions::default(),
                            reply,
                            BasicProperties::default().with_correlation_id(correlation_id),
                        )
                        .await?;
                    delivery.ack(BasicAckOptions::default()).await?;
                }
                Err(_) => delivery.reject(BasicRejectOptions::default()).await?,
            };
        }

        Ok(())
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

#[cfg(test)]
mod drain {
    use super::*;
    use lapin::{
        message::Delivery,
        options::{BasicGetOptions, BasicPublishOptions},
        types::ShortString,
        BasicProperties,
    };
    use uuid::Uuid;

    const URL: &str = "amqp://127.0.0.1:5672";
    const PING: &str = "Ping";
    const PONG: &str = "Pong";

    #[derive(Clone)]
    struct Queue {
        name: String,
        channel: Channel,
    }

    impl Queue {
        async fn new() -> Self {
            let name = Uuid::new_v4().to_string();

            let channel = Connection::connect(URL, ConnectionProperties::default().with_tokio())
                .await
                .unwrap()
                .create_channel()
                .await
                .unwrap();

            channel
                .queue_delete(&name, lapin::options::QueueDeleteOptions::default())
                .await
                .unwrap();
            channel
                .queue_declare(
                    &name,
                    lapin::options::QueueDeclareOptions::default(),
                    lapin::types::FieldTable::default(),
                )
                .await
                .unwrap();

            Self { name, channel }
        }

        async fn publish(&self, payload: Vec<u8>, correlation_id: &str, reply_to: &str) {
            self.channel
                .basic_publish(
                    "",
                    &self.name,
                    BasicPublishOptions::default(),
                    payload,
                    BasicProperties::default()
                        .with_correlation_id(correlation_id.into())
                        .with_reply_to(reply_to.into()),
                )
                .await
                .unwrap();
        }

        async fn get(&self) -> Option<Delivery> {
            match self
                .channel
                .basic_get(&self.name, BasicGetOptions::default())
                .await
                .unwrap()
            {
                Some(m) => Some(m.delivery),
                None => None,
            }
        }
    }

    async fn wait_a_moment() {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    #[tokio::test]
    async fn responds_handler_result_as_data() {
        let queue = Queue::new().await;
        let reply_queue = Queue::new().await;

        let drain_queue = queue.name.clone();
        let server = RmqRpcServer::connect(URL).await.unwrap();
        tokio::spawn(async move {
            server
                .drain(&drain_queue, |_| async {
                    let res: Result<Vec<u8>, std::io::Error> = Ok(PONG.as_bytes().to_vec());
                    res
                })
                .await
                .unwrap();
        });

        queue
            .publish(
                PING.as_bytes().to_vec(),
                "correlation_id",
                &reply_queue.name,
            )
            .await;
        wait_a_moment().await;
        assert_eq!(
            String::from_utf8_lossy(reply_queue.get().await.unwrap().data.as_slice()),
            PONG
        );
    }

    #[tokio::test]
    async fn responds_with_correlation_id() {
        let queue = Queue::new().await;
        let reply_queue = Queue::new().await;

        let drain_queue = queue.name.clone();
        let server = RmqRpcServer::connect(URL).await.unwrap();
        tokio::spawn(async move {
            server
                .drain(&drain_queue, |_| async {
                    let res: Result<Vec<u8>, std::io::Error> = Ok(PONG.as_bytes().to_vec());
                    res
                })
                .await
                .unwrap();
        });

        queue
            .publish(
                PING.as_bytes().to_vec(),
                "correlation_id",
                &reply_queue.name,
            )
            .await;
        wait_a_moment().await;
        assert_eq!(
            reply_queue.get().await.unwrap().properties.correlation_id(),
            &Some(ShortString::from("correlation_id"))
        )
    }

    #[tokio::test]
    async fn processed_message_acked() {
        let queue = Queue::new().await;
        let reply_queue = Queue::new().await;

        let drain_queue = queue.name.clone();
        let server = RmqRpcServer::connect(URL).await.unwrap();
        tokio::spawn(async move {
            server
                .drain(&drain_queue, |_| async {
                    let res: Result<Vec<u8>, std::io::Error> = Ok(PONG.as_bytes().to_vec());
                    res
                })
                .await
                .unwrap();
        });

        queue
            .publish(
                PING.as_bytes().to_vec(),
                "correlation_id",
                &reply_queue.name,
            )
            .await;
        wait_a_moment().await;
        assert_eq!(
            [
                reply_queue.get().await.is_some(),
                reply_queue.get().await.is_some()
            ],
            [true, false]
        )
    }

    #[tokio::test]
    async fn rejects_if_handler_errored() {
        let queue = Queue::new().await;
        let reply_queue = Queue::new().await;

        let drain_queue = queue.name.clone();
        tokio::spawn(async move {
            let server = RmqRpcServer::connect(URL).await.unwrap();
            server
                .drain(&drain_queue, |_| async {
                    Err(std::io::Error::new(std::io::ErrorKind::Other, "Oh no!"))
                })
                .await
                .unwrap();
        });

        queue
            .publish(
                PING.as_bytes().to_vec(),
                "correlation_id",
                &reply_queue.name,
            )
            .await;
        assert_eq!(reply_queue.get().await, None)
    }
}
