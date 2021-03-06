use futures_util::StreamExt;
use lapin::{
    options::{
        BasicAckOptions, BasicConsumeOptions, BasicPublishOptions, BasicRejectOptions,
        QueueDeclareOptions,
    },
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

    pub async fn declare_queue(&self, queue: &str) -> Result<&Self, lapin::Error> {
        self.channel
            .queue_declare(queue, QueueDeclareOptions::default(), FieldTable::default())
            .await?;
        Ok(self)
    }

    pub async fn drain<F, Fut, E>(&self, queue_name: &str, handler: F) -> Result<(), lapin::Error>
    where
        F: Fn(Vec<u8>) -> Fut,
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

            match handler(delivery.data.clone()).await {
                Ok(reply) => {
                    if let (Some(reply_to), Some(correlation_id)) = (
                        delivery.properties.reply_to(),
                        delivery.properties.correlation_id(), // Because client require it
                    ) {
                        self.channel
                            .basic_publish(
                                "",
                                reply_to.as_str(),
                                BasicPublishOptions::default(),
                                reply,
                                BasicProperties::default()
                                    .with_correlation_id(correlation_id.clone()),
                            )
                            .await?;
                    }

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
    use lapin::types::ShortString;
    use test_utils::{wait_a_moment, Queue, PING, PONG, URL};

    type HandlerRet = Result<Vec<u8>, std::io::Error>;

    #[tokio::test]
    async fn responds_with_handler_result() {
        let queue = Queue::new().await;
        let reply_queue = Queue::new().await;
        let queue_to_drain = queue.name.clone();

        let server = RmqRpcServer::connect(URL).await.unwrap();
        tokio::spawn(async move {
            server
                .drain(&queue_to_drain, |_| async {
                    Ok(PONG.as_bytes().to_vec()) as HandlerRet
                })
                .await
                .unwrap();
        });

        queue
            .publish(PING, Some("correlation_id"), Some(&reply_queue.name))
            .await;
        wait_a_moment().await;
        assert_eq!(
            String::from_utf8_lossy(reply_queue.get().await.unwrap().data.as_slice()),
            PONG
        );
    }

    #[tokio::test]
    async fn response_has_correlation_id() {
        let queue = Queue::new().await;
        let reply_queue = Queue::new().await;
        let drain_queue = queue.name.clone();
        let server = RmqRpcServer::connect(URL).await.unwrap();

        tokio::spawn(async move {
            server
                .drain(&drain_queue, |_| async {
                    Ok(PONG.as_bytes().to_vec()) as HandlerRet
                })
                .await
                .unwrap();
        });

        queue
            .publish(PING, Some("correlation_id"), Some(&reply_queue.name))
            .await;
        wait_a_moment().await;
        assert_eq!(
            reply_queue.get().await.unwrap().properties.correlation_id(),
            &Some(ShortString::from("correlation_id"))
        )
    }

    #[tokio::test]
    async fn not_responds_if_reply_to_is_not_specified() {
        let queue = Queue::new().await;
        let drain_queue = queue.name.clone();
        let server = RmqRpcServer::connect(URL).await.unwrap();

        tokio::spawn(async move {
            server
                .drain(&drain_queue, |_| async {
                    Ok(PONG.as_bytes().to_vec()) as HandlerRet
                })
                .await
                .unwrap();
        });

        queue.publish(PING, Some("correlation_id"), None).await;
        wait_a_moment().await;
        assert_eq!(queue.get().await, None)
    }

    #[tokio::test]
    async fn not_responds_if_correlation_id_not_specified() {
        let queue = Queue::new().await;
        let reply_queue = Queue::new().await;
        let drain_queue = queue.name.clone();
        let server = RmqRpcServer::connect(URL).await.unwrap();

        tokio::spawn(async move {
            server
                .drain(&drain_queue, |_| async {
                    Ok(PONG.as_bytes().to_vec()) as HandlerRet
                })
                .await
                .unwrap();
        });

        queue.publish(PING, None, Some(&reply_queue.name)).await;
        wait_a_moment().await;
        assert_eq!(queue.get().await, None)
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
                    Ok(PONG.as_bytes().to_vec()) as HandlerRet
                })
                .await
                .unwrap();
        });

        queue
            .publish(PING, Some("correlation_id"), Some(&reply_queue.name))
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
            .publish(PING, Some("correlation_id"), Some(&reply_queue.name))
            .await;
        assert_eq!(reply_queue.get().await, None)
    }
}
