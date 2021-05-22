use rmq_rpc::RmqRpcServer;
use std::io::Error;

const URL: &str = "amqp://127.0.0.1:5672";
const QUEUE_NAME: &str = "examples";

async fn handler(data: Vec<u8>) -> Result<Vec<u8>, Error> {
    log::info!("Got: {}", String::from_utf8_lossy(&data));

    let mut res = data.to_owned();
    res.reverse();

    log::info!("Respong with: {}", String::from_utf8_lossy(&res));

    Ok(res)
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let server = RmqRpcServer::connect(URL).await.unwrap();

    log::info!("Running rpc server on \"{}\" queue", QUEUE_NAME);
    match server.drain(QUEUE_NAME, handler).await {
        Ok(_) => log::info!("Ok, exiting..."),
        Err(e) => log::info!("{}", e),
    }
}
