use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::{Channel, Receiver, Sender},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum WebSocketIncomingMessage {
    NoteOn(u8),
    NoteOff(u8),
}

pub type WebSocketIncomingChannel = Channel<CriticalSectionRawMutex, WebSocketIncomingMessage, 1>;
pub type WebSocketIncomingSender = Sender<'static, CriticalSectionRawMutex, WebSocketIncomingMessage, 1>;
pub type WebSocketIncomingReceiver = Receiver<'static, CriticalSectionRawMutex, WebSocketIncomingMessage, 1>;
