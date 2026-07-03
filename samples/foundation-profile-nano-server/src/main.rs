use async_opcua_foundation_profile_nano_server::{
    build_server, PROFILE_DISPLAY_NAME, PROFILE_KEY, PROFILE_SURFACE, PROFILE_TARGET_URI,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), String> {
    println!("Starting {PROFILE_DISPLAY_NAME} targeting {PROFILE_TARGET_URI} ({PROFILE_SURFACE})");

    let (server, handle) =
        build_server(format!("pki/foundation-profile-benchmark-{PROFILE_KEY}")).build()?;

    let shutdown = handle.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            shutdown.cancel();
        }
    });

    server.run().await
}
