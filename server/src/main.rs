#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    agent_first::run().await
}
