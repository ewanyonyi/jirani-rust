pub mod auth;
pub mod models;
pub mod routes;
pub mod store;

pub fn rocket() -> rocket::Rocket<rocket::Build> {
    rocket_with_config(auth::GatewayConfig::from_env())
}

pub async fn rocket_from_env() -> Result<rocket::Rocket<rocket::Build>, sqlx::Error> {
    let store = store::GatewayStore::from_env().await?;
    Ok(rocket::build()
        .manage(auth::GatewayConfig::from_env())
        .manage(store)
        .mount("/", routes::routes()))
}

pub fn rocket_with_config(config: auth::GatewayConfig) -> rocket::Rocket<rocket::Build> {
    rocket::build()
        .manage(config)
        .manage(store::GatewayStore::from_memory())
        .mount("/", routes::routes())
}

pub fn rocket_with_store(
    config: auth::GatewayConfig,
    store: store::EnvelopeStore,
) -> rocket::Rocket<rocket::Build> {
    rocket::build()
        .manage(config)
        .manage(store::GatewayStore::from_file_stores(
            store,
            store::RelayBundleStore::default(),
        ))
        .mount("/", routes::routes())
}

pub fn rocket_with_stores(
    config: auth::GatewayConfig,
    envelope_store: store::EnvelopeStore,
    relay_store: store::RelayBundleStore,
) -> rocket::Rocket<rocket::Build> {
    rocket::build()
        .manage(config)
        .manage(store::GatewayStore::from_file_stores(
            envelope_store,
            relay_store,
        ))
        .mount("/", routes::routes())
}

pub fn rocket_with_gateway_store(
    config: auth::GatewayConfig,
    store: store::GatewayStore,
) -> rocket::Rocket<rocket::Build> {
    rocket::build()
        .manage(config)
        .manage(store)
        .mount("/", routes::routes())
}
