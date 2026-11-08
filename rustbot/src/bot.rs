use crate::{
    config::Config,
    dispatcher::Dispatcher,
    modules::{
        afk::AfkModule, eval::EvalModule, exec::ExecModule, help::HelpModule, info::InfoModule,
        notes::NotesModule, ping::PingModule, raw::RawModule, security::SecurityModule,
        settings::SettingsModule, sysinfo::SysinfoModule, translate::TranslateModule,
        updater::UpdaterModule, Module,
    },
    storage::Storage,
};
use anyhow::{bail, Context as _, Result};
use grammers_client::{Client, SignInError, UpdatesConfiguration};
use grammers_mtsender::SenderPool;
use grammers_session::storages::TlSession;
use std::sync::Arc;
use tokio::signal;
use tracing::{error, info};

pub async fn run(config: Arc<Config>) -> Result<()> {
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
    info!("Авторизован как: {} (id={})", me.full_name(), me_id);

    let dispatcher = Arc::new(build_dispatcher(
        Arc::clone(&config),
        Arc::clone(&storage),
        me_id,
        me_username,
    )?);

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
            info!("Ctrl+C — завершаю...");
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
    let mut base_modules: Vec<Arc<dyn Module>> = vec![
        Arc::new(PingModule),
        Arc::new(InfoModule),
        Arc::new(SysinfoModule),
        Arc::new(EvalModule),
        Arc::new(ExecModule),
        Arc::new(RawModule),
        Arc::new(NotesModule),
        Arc::new(AfkModule),
        Arc::new(SettingsModule),
        Arc::new(SecurityModule::new(me_id)),
        Arc::new(TranslateModule),
        Arc::new(UpdaterModule),
    ];

    base_modules.extend(crate::plugins::load_plugins(&config.plugin_dir)?);

    let help = Arc::new(HelpModule::new(&base_modules));
    let mut all = base_modules;
    all.push(help);

    Dispatcher::new(config, storage, me_id, me_username, all)
}

async fn event_loop(
    client: &Client,
    update_stream: &mut grammers_client::client::updates::UpdateStream,
    dispatcher: Arc<Dispatcher>,
) -> Result<()> {
    info!("Бот запущен! Жду команды...");
    loop {
        let update = update_stream.next().await?;
        let dispatcher = Arc::clone(&dispatcher);
        let client = client.clone();
        tokio::spawn(async move {
            if let Err(e) = dispatcher.handle(client, update).await {
                error!("Ошибка диспетчера: {e:#}");
            }
        });
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
