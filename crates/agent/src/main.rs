use agent::runtime::{boot, serve};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    boot::init_logging();
    let (state, config) = boot::boot().await?;
    serve::serve(state, config).await
}
