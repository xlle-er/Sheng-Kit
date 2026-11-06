//! Service clients, one module per vendor family (docs/spec/step4-common.md).
//!
//! Each module exposes the entry points that `ServiceFactory` dispatches to.

pub mod alibaba;
pub mod apple;
pub mod baidu;
pub mod gemini;
pub mod openai;
pub mod tencent;
pub mod text;
pub mod volcano;
pub mod xai;
