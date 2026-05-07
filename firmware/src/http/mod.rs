mod common;
mod web_socket;

use super::http::common::*;
use super::http::web_socket::WebSocketHandler;
use super::types::*;
use alloc::{boxed::Box, vec::Vec};
use embassy_executor::Spawner;
use embassy_net::Stack;
use embassy_time::Duration;
use esp_alloc::ExternalMemory;
use log::info;
use picoserve::{
    make_static,
    response::WebSocketUpgrade,
    routing::{get, post, PathRouter},
    AppBuilder, AppRouter, Router, Server,
};

struct AppProps {
    web_socket_incoming_sender: WebSocketIncomingSender,
}

impl AppProps {
    pub fn new(web_socket_incoming_sender: WebSocketIncomingSender) -> Self {
        Self {
            web_socket_incoming_sender,
        }
    }
}

impl AppBuilder for AppProps {
    type PathRouter = impl PathRouter;

    fn build_app(self) -> Router<Self::PathRouter> {
        Router::from_service(CustomNotFound)
            .route("/", get(async |_: RequestInfo| html_app_response()))
            .nest(
                "/api",
                Router::new()
                    .route(
                        "/reboot",
                        post(async || {
                            esp_hal::system::software_reset();
                            "Unreachable"
                        })
                        .options(async || cors_options_response()),
                    )
                    .route(
                        "/ws",
                        get(async move |upgrade: WebSocketUpgrade| {
                            info!("Upgrade WebSocket connection...");
                            upgrade
                                .on_upgrade(WebSocketHandler::new(self.web_socket_incoming_sender))
                                .with_protocol("json")
                        })
                        .options(async || cors_options_response()),
                    ),
            )
            // Captive Portal stuff...
            .route("/generate_204", get(async || redirect_home_response()))
            .route("/hotspot-detect.html", get(async || redirect_home_response()))
            .route("/connecttest.txt", get(async || redirect_home_response()))
            .route("/redirect", get(async || redirect_home_response()))
    }
}

const WEB_TASK_POOL_SIZE: usize = 4;

static CONFIG: picoserve::Config = picoserve::Config::new(picoserve::Timeouts {
    start_read_request: Duration::from_secs(300),
    persistent_start_read_request: Duration::from_secs(300),
    read_request: Duration::from_secs(300),
    write: Duration::from_secs(300),
});

#[embassy_executor::task(pool_size = WEB_TASK_POOL_SIZE)]
async fn web_task(id: usize, stack: Stack<'static>, app: &'static AppRouter<AppProps>) -> ! {
    info!("Starting Web Task...");

    let port = 80;

    let mut tcp_rx_buffer = Vec::new_in(ExternalMemory);
    tcp_rx_buffer.resize(8 * 1024, 0);
    let mut tcp_tx_buffer = Vec::new_in(ExternalMemory);
    tcp_tx_buffer.resize(8 * 1024, 0);
    let mut http_buffer = Vec::new_in(ExternalMemory);
    http_buffer.resize(8 * 1024, 0);

    Box::new_in(
        Server::new(app, &CONFIG, http_buffer.as_mut())
            .listen_and_serve(
                id,
                stack,
                port,
                tcp_rx_buffer.as_mut_slice(),
                tcp_tx_buffer.as_mut_slice(),
            )
            .await,
        ExternalMemory,
    )
    .into_never()
}

pub fn start_http(spawner: Spawner, stack: Stack<'static>, web_socket_incoming_sender: WebSocketIncomingSender) {
    let app = make_static!(
        AppRouter<AppProps>,
        AppProps::new(web_socket_incoming_sender,).build_app()
    );

    for id in 0..WEB_TASK_POOL_SIZE {
        spawner.spawn(web_task(id, stack, app).unwrap());
    }
}
