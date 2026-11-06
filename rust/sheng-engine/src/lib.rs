//! sheng engine C ABI v1 (`Sources/CShengEngine/include/sheng_engine.h`): engine handles and
//! sessions, asynchronous operations and stateless catalog queries.

pub mod abi;
pub mod factory;
pub mod ffi;
mod operations;
mod probe;
mod registry;
mod runtime;
pub mod session;
#[cfg(feature = "test-support")]
pub mod testing;

// aec-rs-sys links SpeexDSP without a link kind, so it would stay outside the static library
// that Swift links. Naming it static here bundles it into `libsheng_engine.a`.
#[link(name = "speexdsp", kind = "static")]
extern "C" {}
