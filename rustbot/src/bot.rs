use crate::{
    config::Config,
    dispatcher::Dispatcher,
    modules::{
        eval::EvalModule, exec::ExecModule, help::HelpModule, ping::PingModule,
        raw::RawModule, sysinfo::SysinfoModule, Module,
    },
};
use anyhow::{bail, Context as _, Result};
use grammers_client::{Client, SignInError, UpdatesConfiguration};
use grammers_mtsender::SenderPool;
use grammers_session::storages::TlSession;
use std::sync::Arc;
use tokio::signal;
use tracing::{error, info};

pub async fn run(config: Arc<Config>) -> Result<()> {
    // TlSession — синхронная загрузка / создание файла сессии
    let session = Arc::new(
        TlSession::load_file_or_create(&config.session_path)
            .with_context(|| format!("Не удалось открыть сессию '{}'", config.session_path))?,
    );

    info!("Подключаюсь к Telegram...");

    // Создаём пул соединений — Client::new принимает ссылку на пул
    let pool = SenderPool::new(Arc::clone(&session), config.api_id);
    let client = Client::new(&pool);
    let SenderPool { runner, updates, handle } = pool;

    // Запускаем сетевой пул в фоновой задаче
    let pool_task = tokio::spawn(runner.run());

    if !client.is_authorized().await? {
        info!("Требуется авторизация...");
        authorize(&client, &config).await?;
        session
            .save_to_file(&config.session_path)
            .with_context(|| "Не удалось сохранить сессию после входа")?;
    }

    let me = client.get_me().await?;
    info!(
        "Авторизован как: {} (id={})",
        me.full_name(),
        me.bare_id()
    );

    let dispatcher = Arc::new(build_dispatcher(Arc::clone(&config)));

    // stream_updates — НЕ async, возвращает UpdateStream напрямую
    let mut update_stream = client.stream_updates(
        updates,
        UpdatesConfiguration {
            catch_up: false,
            ..Default::default()
        },
    );

    // Главный цикл
    let loop_result = tokio::select! {
        res = event_loop(&client, &mut update_stream, Arc::clone(&dispatcher)) => res,
        _ = signal::ctrl_c() => {
            info!("Получен сигнал завершения (Ctrl+C)");
            Ok(())
        }
    };

    // Синхронизируем состояние обновлений и сохраняем сессию
    update_stream.sync_update_state(); // НЕ async
    if let Err(e) = session.save_to_file(&config.session_path) {
        error!("Не удалось сохранить сессию: {e}");
    } else {
        info!("Сессия сохранена");
    }

    info!("Завершаю соединение...");
    handle.quit();
    let _ = pool_task.await;

    loop_result
}

fn build_dispatcher(config: Arc<Config>) -> Dispatcher {
    let base_modules: Vec<Arc<dyn Module>> = vec![
        Arc::new(PingModule),
        Arc::new(SysinfoModule),
        Arc::new(EvalModule),
        Arc::new(ExecModule),
        Arc::new(RawModule),
    ];

    let help = Arc::new(HelpModule::new(&base_modules));
    let mut all_modules = base_modules;
    all_modules.push(help);

    Dispatcher::new(config, all_modules)
}

async fn event_loop(
    client: &Client,
    update_stream: &mut grammers_client::client::updates::UpdateStream,
    dispatcher: Arc<Dispatcher>,
) -> Result<()> {
    info!("Бот запущен, слушаю обновления...");
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
        .with_context(|| "Не удалось запросить код авторизации")?;

    print!("Введи код из Telegram для {}: ", config.phone);
    std::io::stdout().flush()?;
    let code = read_line()?;

    match client.sign_in(&token, code.trim()).await {
        Ok(_) => {
            info!("Авторизация прошла успешно");
            Ok(())
        }
        Err(SignInError::PasswordRequired(pwd_token)) => {
            print!("Введи пароль двухфакторной аутентификации: ");
            std::io::stdout().flush()?;
            let pwd = read_line()?;
            client
                .check_password(pwd_token, pwd.trim())
                .await
                .with_context(|| "Неверный пароль 2FA")?;
            info!("2FA пройдена успешно");
            Ok(())
        }
        Err(SignInError::InvalidCode) => bail!("Неверный код авторизации"),
        Err(e) => bail!("Ошибка авторизации: {e:?}"),
    }
}

fn read_line() -> Result<String> {
    use std::io::BufRead;
    let stdin = std::io::stdin();
    let mut line = String::new();
    stdin.lock().read_line(&mut line)?;
    Ok(line)
}
