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
| `.ping`              | Проверка времени отклика                    |
| `.sysinfo`           | CPU / RAM / диск / ОС                       |
| `.uptime`            | Аптайм системы                              |
| `.eval <код>`        | Скомпилировать и запустить Rust-сниппет     |
| `.exec <команда>`    | Выполнить команду в bash                    |
| `.id`                | ID чата и отправителя                       |
| `.me`                | Информация о своём аккаунте                 |
| `.d`                 | Удалить сообщение (или ответ на него)       |
| `.cat`               | Показать текст ответного сообщения          |
| `.help [команда]`    | Список всех команд / справка по команде     |

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
