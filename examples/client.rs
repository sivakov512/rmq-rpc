use rmq_rpc::RmqRpcClient;
use std::env;

const URL: &str = "amqp://127.0.0.1:5672";
const QUEUE_NAME: &str = "examples";

#[tokio::main]
async fn main() {
    env_logger::init();
    let msg = match env::args().skip(1).next() {
        Some(msg) => msg,
        None => {
            log::error!("Pass message to send with cli argument");
            return;
        }
    };

    let client = RmqRpcClient::connect(URL).await.unwrap();
    client.declare_queue(QUEUE_NAME).await.unwrap();

    log::info!("Publishing \"{}\" on \"{}\" queue...", msg, QUEUE_NAME);
    let got = client
        .send_message(QUEUE_NAME, msg.as_bytes().to_vec())
        .await
        .unwrap();
    log::info!("Got \"{}\"", String::from_utf8_lossy(&got.data));
}
