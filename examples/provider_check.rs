use job_radar::backend::Backend;
use job_radar::config::Config;
#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let config = Config::load().unwrap();
    let backend = Backend::from_config(&config).unwrap();
    println!("{}", backend.describe());
    let start = std::time::Instant::now();
    match backend.chat("You check if an AI API works. Reply with only the word OK.", "Check").await {
        Ok(r) => println!("OK in {:.1}s: {}", start.elapsed().as_secs_f32(), r.trim()),
        Err(e) => println!("ERROR after {:.1}s: {e}", start.elapsed().as_secs_f32()),
    }
    println!("{:?}", backend.usage());
}
