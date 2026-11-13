//! 🎲 Фан оракул — перенос Hikka-модуля на Rust.
//! Команды: .шипчат .шип .таро .аура .оракул .кто .ктотоп .шанс .монетка

use super::{esc, Command, Context, Module};
use anyhow::Result;
use async_trait::async_trait;
use grammers_client::types::Chat;
use std::collections::{hash_map::RandomState, HashMap, HashSet};
use std::hash::{BuildHasher, Hasher};

pub struct OracleModule;

const TAROT: [(&str, &str); 3] = [
    ("Маг", "Твоя воля — закон."),
    ("Луна", "Тайны откроются."),
    ("Солнце", "Успех близок."),
];
const COLORS: [&str; 3] = ["Синяя", "Индиго", "Алая"];
const VIBES: [&str; 3] = ["Чилл", "Хаос", "Мудрость"];
const ANSWERS: [&str; 4] = ["Да", "Нет", "Вероятно", "Вряд ли"];

// ── случайность ──────────────────────────────────────────────────────────────

/// Случайное число от 0 до n-1 (зерно берётся у ОС).
fn rnd(n: usize) -> usize {
    (RandomState::new().build_hasher().finish() % n as u64) as usize
}

fn pick<T>(items: &[T]) -> &T {
    &items[rnd(items.len())]
}

/// Две разные позиции из n (n >= 2).
fn pair(n: usize) -> (usize, usize) {
    let a = rnd(n);
    (a, (a + 1 + rnd(n - 1)) % n)
}

/// Аура не случайна: у одного человека она одна и та же весь день.
fn aura_for(user_id: i64, date: &str) -> (&'static str, &'static str) {
    let mut h: u64 = 0xcbf29ce484222325; // FNV-1a
    for b in format!("aura_{user_id}_{date}").bytes() {
        h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
    (COLORS[(h >> 32) as usize % 3], VIBES[(h >> 16) as usize % 3])
}

// ── люди и чат ───────────────────────────────────────────────────────────────

struct Person {
    id: i64,
    name: String,
}

impl Person {
    fn new(chat: &Chat) -> Option<Self> {
        let Chat::User(u) = chat else { return None };
        Some(Self {
            id: u.bare_id(),
            name: u.first_name().unwrap_or("Пользователь").to_owned(),
        })
    }

    fn mention(&self) -> String {
        format!("<a href=\"tg://user?id={}\">{}</a>", self.id, esc(&self.name))
    }
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn chat_key(ctx: &Context) -> String {
    ctx.message.chat_id().bot_api_dialog_id().to_string()
}

/// Люди (не боты) из последних 100 сообщений чата.
async fn recent_users(ctx: &Context) -> Result<Vec<Person>> {
    let mut messages = ctx.client.iter_messages(ctx.peer_ref()).limit(100);
    let (mut seen, mut users) = (HashSet::new(), Vec::new());
    while let Some(msg) = messages.next().await? {
        if let Some(Chat::User(u)) = msg.sender() {
            if !u.is_bot() && seen.insert(u.bare_id()) {
                users.extend(msg.sender().and_then(Person::new));
            }
        }
    }
    Ok(users)
}

/// Кого смотрим в `.аура`: автора сообщения, на которое ответили, иначе себя.
async fn aura_target(ctx: &Context) -> Result<Option<Person>> {
    if let Some(id) = ctx.message.reply_to_message_id() {
        let msgs = ctx.client.get_messages_by_id(ctx.peer_ref(), &[id]).await?;
        return Ok(msgs.into_iter().next().flatten().and_then(|m| m.sender().and_then(Person::new)));
    }
    Ok(Person::new(&Chat::User(ctx.client.get_me().await?)))
}

/// Две случайные персоны из чата в виде «A + B = 💘».
async fn random_couple(ctx: &Context) -> Result<Option<String>> {
    let users = recent_users(ctx).await?;
    if users.len() < 2 {
        return Ok(None);
    }
    let (a, b) = pair(users.len());
    Ok(Some(format!("{} + {} = 💘", users[a].mention(), users[b].mention())))
}

// ── топ «кто» ────────────────────────────────────────────────────────────────

fn top_list(counts: &HashMap<String, u64>, names: &HashMap<String, String>) -> String {
    let mut sorted: Vec<_> = counts.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    sorted
        .iter()
        .take(10)
        .enumerate()
        .map(|(i, (uid, n))| {
            let name = names.get(*uid).map_or(format!("ID:{uid}"), |s| esc(s));
            format!("{}. ✅ <b>{name}</b> — <code>{n}</code>\n", i + 1)
        })
        .collect()
}

// ── модуль ───────────────────────────────────────────────────────────────────

#[async_trait]
impl Module for OracleModule {
    fn name(&self) -> &str {
        "oracle"
    }

    fn commands(&self) -> Vec<Command> {
        vec![
            Command::new("шипчат", "Случайная пара прямо сейчас", ".шипчат"),
            Command::new("шип", "Пара дня (раз в сутки)", ".шип"),
            Command::new("таро", "Карта таро", ".таро"),
            Command::new("аура", "Узнать ауру (ответом — чужую)", ".аура"),
            Command::new("оракул", "Вопрос шару", ".оракул <вопрос>"),
            Command::new("кто", "Выбрать человека из чата", ".кто [текст]"),
            Command::new("ктотоп", "Топ везунчиков чата", ".ктотоп"),
            Command::new("шанс", "Шанс события", ".шанс [событие]"),
            Command::new("монетка", "Подбросить монетку", ".монетка"),
        ]
    }

    async fn handle(&self, cmd: &str, ctx: Context) -> Result<()> {
        let arg = ctx.args_raw.trim();
        let text = match cmd {
            "шипчат" => match random_couple(&ctx).await? {
                Some(c) => format!("❤️ <b>Новая пара в чате:</b>\n\n{c}"),
                None => "<b>❌ Недостаточно людей в истории чата.</b>".into(),
            },
            "шип" => {
                let key = format!("oracle.ship.{}.{}", chat_key(&ctx), today());
                let couple = match ctx.storage.get::<String>(&key) {
                    Some(saved) => Some(saved),
                    None => {
                        let new = random_couple(&ctx).await?;
                        if let Some(c) = &new {
                            // пары прошлых дней больше не нужны
                            for old in ctx.storage.keys_with_prefix("oracle.ship.") {
                                if !old.ends_with(&today()) {
                                    let _ = ctx.storage.del(&old);
                                }
                            }
                            ctx.storage.set(&key, c)?;
                        }
                        new
                    }
                };
                match couple {
                    Some(c) => format!("❤️ <b>Пара дня:</b>\n\n{c}"),
                    None => "<b>❌ Мало людей.</b>".into(),
                }
            }
            "таро" => {
                let (card, meaning) = pick(&TAROT);
                format!("🔮 <b>Карта:</b> {card}\n<b>Суть:</b> <i>{meaning}</i>")
            }
            "аура" => match aura_target(&ctx).await? {
                Some(u) => {
                    let (color, vibe) = aura_for(u.id, &today());
                    format!("✨ <b>Аура {}:</b>\n🌈 Цвет: {color}\n🎭 Вайб: {vibe}", u.mention())
                }
                None => "<b>❌ Юзер не найден.</b>".into(),
            },
            "оракул" if arg.is_empty() => "<b>❌ Спроси!</b>".into(),
            "оракул" => format!("🎱 <b>Вопрос:</b> {}\n<b>Ответ:</b> {}", esc(arg), pick(&ANSWERS)),
            "кто" => {
                let users = recent_users(&ctx).await?;
                if users.is_empty() {
                    "<b>❌ Пусто.</b>".into()
                } else {
                    let chosen = &users[rnd(users.len())];
                    let uid = chosen.id.to_string();

                    let mut stats: HashMap<String, HashMap<String, u64>> =
                        ctx.storage.get("oracle.stats").unwrap_or_default();
                    *stats.entry(chat_key(&ctx)).or_default().entry(uid.clone()).or_insert(0) += 1;
                    ctx.storage.set("oracle.stats", &stats)?;

                    // имя запоминаем, чтобы в топе не ходить в API за каждым
                    let mut names: HashMap<String, String> =
                        ctx.storage.get("oracle.names").unwrap_or_default();
                    names.insert(uid, chosen.name.clone());
                    ctx.storage.set("oracle.names", &names)?;

                    let title = if arg.is_empty() { "Выбран:" } else { arg };
                    format!("👤 <b>{}</b> {}", esc(title), chosen.mention())
                }
            }
            "ктотоп" => {
                let stats: HashMap<String, HashMap<String, u64>> =
                    ctx.storage.get("oracle.stats").unwrap_or_default();
                match stats.get(&chat_key(&ctx)).filter(|s| !s.is_empty()) {
                    None => "<b>📊 Пусто.</b>".into(),
                    Some(counts) => {
                        let names = ctx.storage.get("oracle.names").unwrap_or_default();
                        format!("🏆 <b>Топ везунчиков:</b>\n\n{}", top_list(counts, &names))
                    }
                }
            }
            "шанс" => {
                let what = if arg.is_empty() { "события" } else { arg };
                format!("📊 Шанс <i>{}</i>: <b>{}%</b>", esc(what), rnd(101))
            }
            "монетка" => format!("🪙 Выпало: <b>{}</b>", pick(&["Орел", "Решка"])),
            _ => return Ok(()),
        };
        ctx.edit(text).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_helpers_stay_in_range() {
        let mut seen = HashSet::new();
        for _ in 0..300 {
            assert!(rnd(101) <= 100);
            seen.insert(rnd(3));
            let (a, b) = pair(2);
            assert!(a != b && a < 2 && b < 2);
            let (a, b) = pair(7);
            assert!(a != b && a < 7 && b < 7);
        }
        assert_eq!(seen.len(), 3);
    }

    #[test]
    fn aura_is_stable_for_a_day() {
        assert_eq!(aura_for(42, "2026-11-13"), aura_for(42, "2026-11-13"));
        let all: HashSet<_> = (0..60).map(|id| aura_for(id, "2026-11-13")).collect();
        assert!(all.len() > 1);
    }

    #[test]
    fn top_is_sorted_limited_and_escaped() {
        let counts: HashMap<_, _> = (0..15u64).map(|i| (i.to_string(), i + 1)).collect();
        let names = HashMap::from([("14".to_string(), "Макс<".to_string())]);
        let top = top_list(&counts, &names);
        assert_eq!(top.lines().count(), 10);
        assert!(top.starts_with("1. ✅ <b>Макс&lt;</b> — <code>15</code>"));
        assert!(top.contains("ID:13"));
    }

    #[test]
    fn mention_is_escaped() {
        let p = Person { id: 7, name: "A<&>".into() };
        assert_eq!(p.mention(), "<a href=\"tg://user?id=7\">A&lt;&amp;&gt;</a>");
    }

    #[test]
    fn commands_are_dispatchable() {
        for c in OracleModule.commands() {
            let line = format!(".{} x", c.name);
            let (cmd, _) = crate::dispatcher::parse_command(&line, '.').expect(&c.name);
            assert_eq!(cmd, c.name);
        }
    }
}
