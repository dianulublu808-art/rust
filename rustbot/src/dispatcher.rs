use crate::{
    config::Config,
    modules::{Context, Module},
};
use anyhow::Result;
use grammers_client::Client;
use grammers_client::update::Update;
use std::sync::Arc;
use tracing::{debug, error, warn};

pub struct Dispatcher {
    modules: Vec<Arc<dyn Module>>,
    config: Arc<Config>,
}

impl Dispatcher {
    pub fn new(config: Arc<Config>, modules: Vec<Arc<dyn Module>>) -> Self {
        for m in &modules {
            for c in m.commands() {
                debug!("Registered command '{}' from module '{}'", c.name, m.name());
            }
        }
        Self { modules, config }
    }

    /// Обрабатывает входящее обновление.
    pub async fn handle(&self, client: Client, update: Update) -> Result<()> {
        let msg = match update {
            Update::NewMessage(m) => m,
            Update::MessageEdited(m) => m,
            _ => return Ok(()),
        };

        // Реагируем только на исходящие сообщения (от себя)
        if !msg.outgoing() {
            return Ok(());
        }

        let text = msg.text().to_owned();
        let prefix = self.config.prefix_char();

        if !text.starts_with(prefix) {
            return Ok(());
        }

        let without_prefix = text[prefix.len_utf8()..].trim_start();
        if without_prefix.is_empty() {
            return Ok(());
        }

        let mut parts = without_prefix.splitn(2, char::is_whitespace);
        let cmd = match parts.next() {
            Some(c) if !c.is_empty() => c.to_lowercase(),
            _ => return Ok(()),
        };
        let args_raw = parts.next().unwrap_or("").to_owned();
        let args: Vec<String> = args_raw.split_whitespace().map(str::to_owned).collect();

        let handler = self.modules.iter().find(|m| {
            m.commands()
                .iter()
                .any(|c| c.name.eq_ignore_ascii_case(&cmd))
        });

        match handler {
            None => {
                warn!("Unknown command: '{cmd}'");
                Ok(())
            }
            Some(module) => {
                let ctx = Context {
                    client,
                    message: msg,
                    args,
                    args_raw,
                    config: Arc::clone(&self.config),
                };
                if let Err(e) = module.handle(&cmd, ctx).await {
                    error!(
                        module = module.name(),
                        cmd = %cmd,
                        "Module error: {e:#}"
                    );
                }
                Ok(())
            }
        }
    }
}
