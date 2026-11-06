//! Session runtime: request configuration, catalogs, network transport, audio queues and
//! echo processing, provider abstractions and the text translation pipeline.
//! Service clients live under `services`, one module per vendor family; modules whose
//! protocol is not implemented yet report `provider_configuration_failed`.

pub mod audio;
pub mod catalog;
pub mod config;
pub mod pipeline;
pub mod provider;
pub mod services;
pub mod transport;
