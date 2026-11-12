#!/usr/bin/env bash
# Запуск бота с автоперезапуском при падении (например, если Android убил процесс).
#   ./start.sh              — обычный запуск
#   ./start.sh другой.toml  — с другим конфигом
# Штатная остановка (Ctrl+C, pkill rustbot) цикл завершает.
cd "$(dirname "$0")" || exit 1

BIN=./target/release/rustbot
if [ ! -x "$BIN" ]; then
  echo "Нет $BIN. Сначала соберите: cargo build --release -j1"
  exit 1
fi

command -v termux-wake-lock >/dev/null 2>&1 && termux-wake-lock

delay=3
while true; do
  started=$(date +%s)
  "$BIN" "$@"
  code=$?
  if [ "$code" -eq 0 ]; then
    echo "Бот остановлен."
    exit 0
  fi
  ran=$(( $(date +%s) - started ))
  [ "$ran" -ge 120 ] && delay=3
  echo "⚠ Бот завершился с кодом $code (проработал ${ran} с). Перезапуск через ${delay} с (Ctrl+C — отмена)"
  sleep "$delay"
  delay=$(( delay * 2 ))
  [ "$delay" -gt 60 ] && delay=60
done
