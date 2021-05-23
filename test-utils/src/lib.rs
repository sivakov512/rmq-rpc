use futures_util::StreamExt;
use lapin::{
    message::Delivery,
    options::{
        BasicConsumeOptions, BasicGetOptions, BasicPublishOptions, QueueDeclareOptions,
        QueueDeleteOptions,
    },
    types::FieldTable,
    BasicProperties, Channel, Connection, ConnectionProperties,
};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_amqp::*;
use uuid::Uuid;

pub const URL: &str = "amqp://127.0.0.1:5672";
pub const PING: &str = "Ping";
pub const PONG: &str = "Pong";

#[derive(Clone)]
pub struct Queue {
    pub name: String,
    channel: Channel,
    processed_deliveries: Arc<Mutex<Vec<Delivery>>>,
}

impl Queue {
    pub async fn new() -> Self {
        let name = Uuid::new_v4().to_string();

        let channel = Connection::connect(URL, ConnectionProperties::default().with_tokio())
            .await
            .unwrap()
            .create_channel()
            .await
            .unwrap();

        channel
            .queue_delete(&name, QueueDeleteOptions::default())
            .await
            .unwrap();
        channel
            .queue_declare(&name, QueueDeclareOptions::default(), FieldTable::default())
            .await
            .unwrap();

        Self {
            name,
            channel,
            processed_deliveries: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub async fn publish(
        &self,
        payload: &str,
        correlation_id: Option<&str>,
        reply_to: Option<&str>,
    ) {
        let mut properties = BasicProperties::default();
        if let Some(correlation_id) = correlation_id {
            properties = properties.with_correlation_id(correlation_id.into())
        }
        if let Some(reply_to) = reply_to {
            properties = properties.with_reply_to(reply_to.into())
        }

        self.channel
            .basic_publish(
                "",
                &self.name,
                BasicPublishOptions::default(),
                payload.as_bytes().to_vec(),
                properties,
            )
            .await
            .unwrap();
    }

    pub async fn get(&self) -> Option<Delivery> {
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

    pub async fn with_auto_reply() -> Self {
        let queue = Self::new().await;
        let cloned = queue.clone();

        tokio::spawn(async move {
            let mut consumer = cloned
                .channel
                .basic_consume(
                    &cloned.name,
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
                cloned
                    .processed_deliveries
                    .lock()
                    .await
                    .push(delivery.clone());

                let reply_to = delivery.properties.reply_to().clone().unwrap().to_string();
                cloned
                    .channel
                    .basic_publish(
                        "",
                        &reply_to,
                        BasicPublishOptions::default(),
                        PONG.as_bytes().to_vec(),
                        BasicProperties::default().with_correlation_id(
                            delivery.properties.correlation_id().clone().unwrap(),
                        ),
                    )
                    .await
                    .unwrap();
            }
        });

        queue
    }

    pub async fn spawn_server(with_auto_reply: bool) -> Self {
        let queue = Self::new().await;
        let cloned = queue.clone();

        tokio::spawn(async move {
            let mut consumer = cloned
                .channel
                .basic_consume(
                    &cloned.name,
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
                cloned
                    .processed_deliveries
                    .lock()
                    .await
                    .push(delivery.clone());

                if with_auto_reply {
                    let reply_to = delivery.properties.reply_to().clone().unwrap().to_string();

                    cloned
                        .channel
                        .basic_publish(
                            "",
                            &reply_to,
                            BasicPublishOptions::default(),
                            PONG.as_bytes().to_vec(),
                            BasicProperties::default().with_correlation_id(
                                delivery.properties.correlation_id().clone().unwrap(),
                            ),
                        )
                        .await
                        .unwrap();
                }
            }
        });

        queue
    }

    pub async fn processed_deliveries(&self) -> Vec<Delivery> {
        self.processed_deliveries.lock().await.clone()
    }
}

pub async fn wait_a_moment() {
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
}
