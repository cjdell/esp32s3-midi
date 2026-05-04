use crate::{types::*, utils::*};
use alloc::vec::Vec;
use esp_alloc::ExternalMemory;
use picoserve::{
    futures::Either,
    response::ws::{Message, SocketRx, SocketTx, WebSocketCallback},
};

pub struct WebSocketHandler {
    web_socket_incoming_sender: WebSocketIncomingSender,
}

impl WebSocketHandler {
    pub fn new(web_socket_incoming_sender: WebSocketIncomingSender) -> Self {
        Self {
            web_socket_incoming_sender,
        }
    }
}

impl WebSocketCallback for WebSocketHandler {
    async fn run<R: picoserve::io::Read, W: picoserve::io::Write<Error = R::Error>>(
        self,
        mut rx: SocketRx<R>,
        mut tx: SocketTx<W>,
    ) -> Result<(), W::Error> {
        use Message;

        log::info!("WebSocket closed");

        let mut message_buffer = Vec::new_in(ExternalMemory);
        message_buffer.resize(4096, 0u8);

        let close_reason = loop {
            let message = match rx.next_message(&mut message_buffer, sleep(1_000_000)).await? {
                Either::First(Ok(message)) => message,
                Either::First(Err(error)) => {
                    log::warn!("Websocket error: {error:?}");
                    break Some((error.code(), "Websocket Error"));
                }
                Either::Second(()) => {
                    continue;
                }
            };

            log::info!("Message: {message:?}");

            match message {
                Message::Text(message) => {
                    let message = match serde_json::from_str(message) {
                        Ok(message) => message,
                        Err(err) => {
                            log::error!("Serde Error: {err:?}");
                            continue;
                        }
                    };
                    self.web_socket_incoming_sender.send(message).await;
                }
                Message::Binary(message) => {
                    let message = match serde_json::from_slice(message) {
                        Ok(message) => message,
                        Err(err) => {
                            log::error!("Serde Error: {err:?}");
                            continue;
                        }
                    };
                    self.web_socket_incoming_sender.send(message).await;
                }
                Message::Close(reason) => {
                    log::info!("Websocket close reason: {reason:?}");
                    break None;
                }
                Message::Ping(ping) => tx.send_pong(ping).await?,
                Message::Pong(_) => (),
            };
        };

        let close_fut = tx.close(close_reason).await;

        log::info!("WebSocket closed");

        close_fut
    }
}
