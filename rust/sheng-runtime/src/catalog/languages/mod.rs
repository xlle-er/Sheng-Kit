//! Language sets per speech route and text translator, rebuilt from vendor documentation.
//! Each set notes its source; differences from the recorded catalog are in docs/spec/catalog-diff.md
//! and docs/spec/catalog-diff/*.md.
//! Split by service family so each step 4 sub-step owns one file (docs/spec/step4-common.md).

pub mod alibaba;
pub mod apple;
pub mod baidu;
pub mod gemini;
pub mod openai;
pub mod tencent;
pub mod text;
pub mod volcano;
pub mod xai;

pub use alibaba::*;
pub use baidu::*;
pub use gemini::*;
pub use openai::*;
pub use tencent::*;
pub use text::*;
pub use volcano::*;
pub use xai::*;
