use crate::{
    config::Config,
    dispatcher::Dispatcher,
    modules::{
        afk::AfkModule, botconfig::BotConfigModule, eval::EvalModule, exec::ExecModule, help::HelpModule, info::InfoModule,
        notes::NotesModule, ping::PingModule, raw::RawModule, security::SecurityModule,
        reload::ReloadModule, settings::SettingsModule, sysinfo::SysinfoModule,
        translate::TranslateModule, updater::UpdaterModule, Module, Registry,
    },
    storage::Storage,
};
use anyhow::{bail, Context as _, Result};
use grammers_client::{Client, SignInError, UpdatesConfiguration};
use grammers_mtsender::SenderPool;
use grammers_session::storages::TlSession;
use std::sync::{Arc, RwLock};
use tokio::signal;
use std::time::Duration;
use tracing::{error, info, warn};

pub async fn run(config: Arc<Config>) -> Result<()> {
    crate::modules::ui::init();
    let session = Arc::new(
        TlSession::load_file_or_create(&config.session_path)
            .with_context(|| format!("Не удалось открыть сессию '{}'", config.session_path))?,
    );

    let storage = Arc::new(
        Storage::load(&config.storage_path)
            .with_context(|| "Не удалось загрузить хранилище данных")?,
    );

    info!("Подключаюсь к Telegram...");

    let pool = SenderPool::new(Arc::clone(&session), config.api_id);
    let client = Client::new(&pool);
    let SenderPool {
        runner,
        updates,
        handle,
    } = pool;
    let pool_task = tokio::spawn(runner.run());

    if !client.is_authorized().await? {
        info!("Требуется авторизация...");
        authorize(&client, &config).await?;
        session
            .save_to_file(&config.session_path)
            .with_context(|| "Не удалось сохранить сессию после входа")?;
    }

    let me = client.get_me().await?;
    let me_id = me.bare_id();
    let me_username = me.username().map(str::to_owned);

    // Без кэша пиров (access_hash каналов/групп) обновления из них теряются —
    // отсюда "cannot getChannelDifference ... missing its hash".
    let dialogs = warm_peer_cache(&client).await;
    if let Err(e) = session.save_to_file(&config.session_path) {
        warn!("Не удалось сохранить сессию: {e}");
    }
    {
        // Сессию сохраняем периодически, а не только при выходе:
        // Termux/Android может убить процесс, и кэш пиров пропадёт.
        let session = Arc::clone(&session);
        let path = config.session_path.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(120));
            tick.tick().await;
            loop {
                tick.tick().await;
                if let Err(e) = session.save_to_file(&path) {
                    warn!("Не удалось сохранить сессию: {e}");
                }
            }
        });
    }

    let who = match &me_username {
        Some(u) => format!("{} (@{u})", me.full_name()),
        None => me.full_name(),
    };
    let prefix = storage
        .get::<String>("settings.prefix")
        .and_then(|p| p.chars().next())
        .unwrap_or_else(|| config.prefix_char());

    let dispatcher = Arc::new(build_dispatcher(
        Arc::clone(&config),
        Arc::clone(&storage),
        me_id,
        me_username,
    )?);

    let (modules, commands) = dispatcher.stats();
    crate::term::info_block(
        "Сессия",
        &[
            ("Аккаунт", crate::term::paint("1", &who)),
            ("ID", me_id.to_string()),
            ("Префикс", crate::term::paint("1;38;5;208", prefix)),
            ("Модули", format!("{modules} · команд {commands}")),
            (
                "Диалоги",
                match dialogs {
                    Some(n) => format!("{n} в кэше"),
                    None => crate::term::paint("93", "не загружены"),
                },
            ),
        ],
        &format!(
            "{} {}",
            crate::term::paint("1;92", "онлайн"),
            crate::term::dim("· жду команды · Ctrl+C — выход")
        ),
    );

    let mut update_stream = client.stream_updates(
        updates,
        UpdatesConfiguration {
            catch_up: false,
            ..Default::default()
        },
    );

    let loop_result = tokio::select! {
        res = event_loop(&client, &mut update_stream, Arc::clone(&dispatcher)) => res,
        _ = signal::ctrl_c() => {
            println!();
            info!("Ctrl+C — завершаю, сохраняю сессию...");
            Ok(())
        }
    };

    update_stream.sync_update_state();
    if let Err(e) = session.save_to_file(&config.session_path) {
        error!("Не удалось сохранить сессию: {e}");
    }

    handle.quit();
    let _ = pool_task.await;
    loop_result
}

fn build_dispatcher(
    config: Arc<Config>,
    storage: Arc<Storage>,
    me_id: i64,
    me_username: Option<String>,
) -> Result<Dispatcher> {
    let base_modules: Vec<Arc<dyn Module>> = vec![
        Arc::new(PingModule),
        Arc::new(InfoModule),
        Arc::new(SysinfoModule),
        Arc::new(EvalModule),
        Arc::new(ExecModule),
        Arc::new(RawModule),
        Arc::new(NotesModule),
        Arc::new(AfkModule),
        Arc::new(SettingsModule),
        Arc::new(BotConfigModule),
        Arc::new(SecurityModule::new(me_id)),
        Arc::new(TranslateModule),
        Arc::new(UpdaterModule),
    ];

    let registry: Registry = Arc::new(RwLock::new(Vec::new()));
    let mut all = base_modules;
    all.push(Arc::new(HelpModule::new(Arc::clone(&registry))));
    let core_count = all.len() + 1;
    all.push(Arc::new(ReloadModule::new(
        Arc::clone(&registry),
        config.plugin_dir.clone(),
        core_count,
    )));
    all.extend(crate::plugins::load_plugins(&config.plugin_dir)?);
    *registry.write().unwrap() = all;

    Dispatcher::new(config, storage, me_id, me_username, registry)
}

async fn event_loop(
    client: &Client,
    update_stream: &mut grammers_client::client::updates::UpdateStream,
    dispatcher: Arc<Dispatcher>,
) -> Result<()> {
    let mut errors = 0u32;
    loop {
        let update = match update_stream.next().await {
            Ok(u) => {
                errors = 0;
                u
            }
            Err(e) => {
                errors += 1;
                error!("Ошибка потока обновлений ({errors}/10): {e}");
                if errors >= 10 {
                    return Err(e.into());
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
        };
        let dispatcher = Arc::clone(&dispatcher);
        let client = client.clone();
        tokio::spawn(async move {
            if let Err(e) = dispatcher.handle(client, update).await {
                error!("Ошибка диспетчера: {e:#}");
            }
        });
    }
}

async fn warm_peer_cache(client: &Client) -> Option<usize> {
    let fut = async {
        let mut dialogs = client.iter_dialogs();
        let mut n = 0usize;
        while dialogs.next().await?.is_some() {
            n += 1;
            if n >= 2000 {
                break;
            }
        }
        Ok::<usize, grammers_client::InvocationError>(n)
    };
    match tokio::time::timeout(Duration::from_secs(90), fut).await {
        Ok(Ok(n)) => Some(n),
        Ok(Err(e)) => {
            warn!("Не удалось загрузить список чатов: {e}");
            None
        }
        Err(_) => {
            warn!("Таймаут загрузки списка чатов");
            None
        }
    }
}

async fn authorize(client: &Client, config: &Config) -> Result<()> {
    use std::io::Write;

    let token = client
        .request_login_code(&config.phone, &config.api_hash)
        .await
        .with_context(|| "Не удалось запросить код")?;

    print!("Код из Telegram для {}: ", config.phone);
    std::io::stdout().flush()?;
    let code = read_line()?;

    match client.sign_in(&token, code.trim()).await {
        Ok(_) => Ok(()),
        Err(SignInError::PasswordRequired(tok)) => {
            print!("Пароль 2FA: ");
            std::io::stdout().flush()?;
            let pwd = read_line()?;
            client
                .check_password(tok, pwd.trim())
                .await
                .with_context(|| "Неверный пароль 2FA")?;
            Ok(())
        }
        Err(SignInError::InvalidCode) => bail!("Неверный код"),
        Err(e) => bail!("Ошибка авторизации: {e:?}"),
    }
}

fn read_line() -> Result<String> {
    use std::io::BufRead;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(line)
}
