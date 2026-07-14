use crate::{
    config::Config,
    dispatcher::Dispatcher,
    modules::{
        eval::EvalModule, exec::ExecModule, help::HelpModule, ping::PingModule,
        raw::RawModule, sysinfo::SysinfoModule, Module,
    },
};
use anyhow::{bail, Context as _, Result};
use grammers_client::{Client, Config as ClientConfig, InitParams, SignIn};
use grammers_session::Session;
use std::sync::Arc;
use tokio::signal;
use tracing::{error, info};

pub async fn run(config: Arc<Config>) -> Result<()> {
    let session = Session::load_file_or_create(&config.session_path)
        .with_context(|| format!("Не удалось загрузить сессию '{}'", config.session_path))?;

    info!("Подключаюсь к Telegram...");

    let client = Client::connect(ClientConfig {
        session,
        api_id: config.api_id,
        api_hash: config.api_hash.clone(),
        params: InitParams {
            app_version: concat!(env!("CARGO_PKG_NAME"), " ", env!("CARGO_PKG_VERSION"))
                .to_string(),
            device_model: "RustBot".to_string(),
            system_version: "Linux".to_string(),
            ..Default::default()
        },
    })
    .await
    .with_context(|| "Не удалось подключиться к Telegram")?;

    if !client.is_authorized().await? {
        info!("Требуется авторизация...");
        authorize(&client, &config).await?;
    }

    let me = client.get_me().await?;
    info!(
        "Авторизован как: {} (id={})",
        me.full_name(),
        me.id()
    );

    let dispatcher = Arc::new(build_dispatcher(Arc::clone(&config)));

    // Главный цикл
    let loop_result = tokio::select! {
        res = event_loop(&client, Arc::clone(&dispatcher)) => res,
        _ = signal::ctrl_c() => {
            info!("Получен сигнал завершения (Ctrl+C)");
            Ok(())
        }
    };

    // Сохраняем сессию при выходе
    if let Err(e) = client.session().save_to_file(&config.session_path) {
        error!("Не удалось сохранить сессию: {e}");
    } else {
        info!("Сессия сохранена");
    }

    loop_result
}

/// Регистрирует все модули и строит диспетчер
fn build_dispatcher(config: Arc<Config>) -> Dispatcher {
    // Сначала регистрируем все модули кроме help
    let base_modules: Vec<Arc<dyn Module>> = vec![
        Arc::new(PingModule),
        Arc::new(SysinfoModule),
        Arc::new(EvalModule),
        Arc::new(ExecModule),
        Arc::new(RawModule),
    ];

    // HelpModule получает список всех команд
    let help = Arc::new(HelpModule::new(&base_modules));

    let mut all_modules = base_modules;
    all_modules.push(help);

    Dispatcher::new(config, all_modules)
}

async fn event_loop(client: &Client, dispatcher: Arc<Dispatcher>) -> Result<()> {
    info!("Бот запущен, слушаю обновления...");
    loop {
        let update = client.next_update().await?;
        let dispatcher = Arc::clone(&dispatcher);
        let client = client.clone();

        // Каждое обновление обрабатывается в отдельной задаче,
        // чтобы одна медленная команда не блокировала другие
        tokio::spawn(async move {
            if let Err(e) = dispatcher.handle(client, update).await {
                error!("Ошибка диспетчера: {e:#}");
            }
        });
    }
}

async fn authorize(client: &Client, config: &Config) -> Result<()> {
    use std::io::{self, BufRead, Write};

    let token = client
        .request_login_code(&config.phone)
        .await
        .with_context(|| "Не удалось запросить код авторизации")?;

    print!("Введи код из Telegram для {}: ", config.phone);
    io::stdout().flush()?;

    let code = read_line()?;

    match client.sign_in(&token, code.trim()).await {
        Ok(_) => {
            info!("Авторизация прошла успешно");
            Ok(())
        }
        Err(SignIn::PasswordRequired(pwd_token)) => {
            print!("Введи пароль двухфакторной аутентификации: ");
            io::stdout().flush()?;
            let pwd = read_line()?;
            client
                .check_password(pwd_token, pwd.trim())
                .await
                .with_context(|| "Неверный пароль 2FA")?;
            info!("2FA пройдена успешно");
            Ok(())
        }
        Err(SignIn::InvalidCode) => bail!("Неверный код авторизации"),
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
