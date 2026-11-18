use sqlx::Connection;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base = std::env::var("TEST_DATABASE_URL")?;
    let (prefix, _) = base
        .rsplit_once('/')
        .ok_or("TEST_DATABASE_URL needs a path")?;
    let name = format!(
        "gw_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis()
    );
    let mut conn = sqlx::MySqlConnection::connect(&base).await?;
    sqlx::query(&format!("create database {name}"))
        .execute(&mut conn)
        .await?;
    println!("{prefix}/{name}");
    Ok(())
}
