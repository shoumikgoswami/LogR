pub mod browser;
pub mod clipboard;
pub mod context;
pub mod filesystem;
pub mod keyboard;
pub mod screenshot;
pub mod window;

use async_trait::async_trait;
use tokio::sync::mpsc;

pub use crate::session::types::RawEvent;

#[async_trait]
pub trait Collector: Send + Sync {
    fn name(&self) -> &str;
    async fn start(&self, tx: mpsc::Sender<RawEvent>);
}
