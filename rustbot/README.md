# RustBot — Telegram Userbot на Rust

## Стек
- [grammers](https://github.com/Lonami/grammers) — MTProto-клиент
- tokio — async runtime
- sysinfo — системные метрики
- tracing — структурированные логи

## Установка

```bash
# 1. Скопируй конфиг
cp config.toml.example config.toml

# 2. Заполни api_id, api_hash, phone на https://my.telegram.org/apps

# 3. Собери и запусти
cargo run --release

# 4. При первом запуске — введи код из Telegram и (если нужно) 2FA-пароль
```

## Команды

| Команда              | Описание                                    |
|----------------------|---------------------------------------------|
| `.ping`                 | Проверка времени отклика                   |
| `.info`, `.alive`       | Информация о боте и проверка работы        |
| `.sysinfo`, `.uptime`   | CPU / RAM / диск / ОС и аптайм системы     |
| `.afk [причина]`        | Включить автоответ при упоминании          |
| `.unafk`                | Выключить AFK                              |
| `.setprefix <знак>`      | Изменить префикс команд (`reset` — сбросить) |
| `.prefix`                | Показать текущий префикс                   |
| `.blacklistchat` / `.unblacklistchat` | Игнорировать чат или снять блокировку |
| `.blacklistuser <id>` / `.unblacklistuser <id>` | Игнорировать или снова отвечать пользователю |
| `.blacklist`             | Показать блок-лист                         |
| `.save <имя> <текст>`   | Сохранить заметку (или сохранить ответ)    |
| `.get <имя>`, `.notes`  | Получить заметку и список заметок         |
| `.delnote <имя>`        | Удалить заметку                            |
| `.tr [язык] [текст]`    | Перевести текст или сообщение-ответ        |
| `.eval <код>` / `.e`    | Скомпилировать и запустить Rust-сниппет    |
| `.exec <команда>`       | Выполнить команду в bash                   |
| `.id`, `.me`, `.d`, `.cat` | ID, профиль, удаление и чтение ответа   |
| `.update [check\|ветка]`, `.restart` | Обновление из Git (с откатом при ошибке сборки) и перезапуск |
| `.reload`               | Перезагрузить WASM-плагины без перезапуска |
| `.plugins`              | Список загруженных плагинов                |
| `.config [ключ] [значение\|reset]` | Настройки с валидацией (таймауты, язык `.tr`) |
| `.help [команда]`       | Список всех команд / справка по команде    |

## WASM-плагины на Rust

Плагины кладутся в каталог `plugins/`: один TOML-манифест и один `.wasm` файл на плагин.
Они исполняются в Wasmi без WASI, импорта функций хоста, доступа к сети или файлам.
На каждый вызов ограничены расход инструкций и память; плагин возвращает текст,
который бот отправляет редактированием исходного сообщения.

В репозитории есть готовый пример в `plugin-examples/hello`. Внутри каталога `rustbot`:

```bash
rustup target add wasm32-unknown-unknown
mkdir -p plugins
cargo build --manifest-path plugin-examples/hello/Cargo.toml \
  --target wasm32-unknown-unknown --release
cp plugin-examples/hello/plugin.toml plugins/hello.toml
cp plugin-examples/hello/target/wasm32-unknown-unknown/release/hello_wasm_plugin.wasm \
  plugins/hello.wasm
cargo run --release -- config.toml
```

После перезапуска команда `.hello [имя]` появится в `.help`. Манифест читается при
старте; для включения или обновления плагина нужно перезапустить бот. Плагины должны
экспортировать `memory`, `alloc(i32) -> i32` и `handle(i32, i32) -> i64`.
Функция `handle` принимает JSON `{command,args,args_raw,message_text,message_id}` и
возвращает упакованные указатель и длину JSON `{text:"ответ"}`.

## Добавить свой модуль

1. Создай `src/modules/my_module.rs`
2. Реализуй трейт `Module`
3. Добавь в `bot.rs` в вектор `base_modules`

```rust
pub struct MyModule;

#[async_trait]
impl Module for MyModule {
    fn name(&self) -> &'static str { "mymodule" }

    fn commands(&self) -> &[Command] {
        &[Command {
            name: "hello",
            description: "Приветствие",
            usage: ".hello",
        }]
    }

    async fn handle(&self, _cmd: &str, ctx: Context) -> Result<()> {
        ctx.edit("Привет!").await
    }
}
```

## Уровень логов

```bash
RUST_LOG=rustbot=debug cargo run
```
