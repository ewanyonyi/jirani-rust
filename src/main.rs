mod auth;
mod models;
mod routes;
mod store;

#[rocket::main]
async fn main() -> Result<(), rocket::Error> {
    rocket::build()
        .manage(auth::GatewayConfig::from_env())
        .manage(store::EnvelopeStore::from_env())
        .manage(store::RelayBundleStore::from_env())
        .mount("/", routes::routes())
        .launch()
        .await?;

    Ok(())
}
