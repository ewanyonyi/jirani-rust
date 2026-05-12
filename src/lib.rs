pub mod auth;
pub mod models;
pub mod routes;
pub mod store;

pub fn rocket() -> rocket::Rocket<rocket::Build> {
    rocket::build()
        .manage(auth::GatewayConfig::from_env())
        .manage(store::EnvelopeStore::from_env())
        .manage(store::RelayBundleStore::from_env())
        .mount("/", routes::routes())
}

pub fn rocket_with_config(config: auth::GatewayConfig) -> rocket::Rocket<rocket::Build> {
    rocket::build()
        .manage(config)
        .manage(store::EnvelopeStore::default())
        .manage(store::RelayBundleStore::default())
        .mount("/", routes::routes())
}

pub fn rocket_with_store(
    config: auth::GatewayConfig,
    store: store::EnvelopeStore,
) -> rocket::Rocket<rocket::Build> {
    rocket::build()
        .manage(config)
        .manage(store)
        .manage(store::RelayBundleStore::default())
        .mount("/", routes::routes())
}

pub fn rocket_with_stores(
    config: auth::GatewayConfig,
    envelope_store: store::EnvelopeStore,
    relay_store: store::RelayBundleStore,
) -> rocket::Rocket<rocket::Build> {
    rocket::build()
        .manage(config)
        .manage(envelope_store)
        .manage(relay_store)
        .mount("/", routes::routes())
}
