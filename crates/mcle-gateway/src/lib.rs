#![forbid(unsafe_code)]

mod detect;
mod internal;
mod pipeline;
mod server;

pub use server::{Gateway, GatewayConfig, Plugins, RunningGateway};
