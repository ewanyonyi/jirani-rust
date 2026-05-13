mod auth;
mod models;
mod routes;
mod store;

#[rocket::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    if args
        .get(1)
        .is_some_and(|arg| arg == "hash-dashboard-password")
    {
        let Some(password) = args.get(2) else {
            eprintln!("Usage: jirani-rust hash-dashboard-password <password> [salt-seed]");
            std::process::exit(2);
        };
        let salt_seed = args
            .get(3)
            .cloned()
            .unwrap_or_else(|| format!("{}:{}", now_epoch_seconds(), std::process::id()));
        println!(
            "{}",
            auth::dashboard_password_hash_for_config(password, &salt_seed)
        );
        return Ok(());
    }

    let store = store::GatewayStore::from_env().await?;

    rocket::build()
        .manage(auth::GatewayConfig::from_env())
        .manage(store)
        .mount("/", routes::routes())
        .launch()
        .await?;

    Ok(())
}

fn now_epoch_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}
