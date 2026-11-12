#!/usr/bin/env bash
# Обновление из git и пересборка одной командой.
#   ./update.sh            — обновить текущую ветку
#   ./update.sh имя-ветки  — перейти на другую ветку и собрать её
#   JOBS=2 ./update.sh     — собирать в 2 потока (по умолчанию 1 — меньше памяти)
# Работает и в неглубоком клоне (fetch + fast-forward, без `git pull`).
set -euo pipefail
cd "$(dirname "$0")"
REPO=$(git rev-parse --show-toplevel)
APP=$(pwd)
cd "$REPO"

CUR=$(git rev-parse --abbrev-ref HEAD)
BRANCH="${1:-$CUR}"
OLD=$(git rev-parse HEAD)

echo "→ git fetch origin $BRANCH"
git fetch origin "$BRANCH"
NEW=$(git rev-parse FETCH_HEAD)

CHANGED=0
if [ "$BRANCH" = "$CUR" ]; then
  if [ "$OLD" != "$NEW" ]; then
    git merge --ff-only FETCH_HEAD
    CHANGED=1
  fi
else
  git checkout -B "$BRANCH" FETCH_HEAD
  CHANGED=1
fi

HEAD_SHORT=$(git rev-parse --short HEAD)
BIN="$APP/target/release/rustbot"

# Пересобираем, если код изменился ИЛИ бинарник собран из другого коммита / отсутствует.
if [ "$CHANGED" -eq 0 ] && [ -x "$BIN" ] && "$BIN" --version 2>/dev/null | grep -q "$HEAD_SHORT"; then
  echo "✓ Уже актуально: $HEAD_SHORT, бинарник собран из него."
  exit 0
fi

echo "→ сборка $HEAD_SHORT (JOBS=${JOBS:-1})"
cd "$APP"
if ! cargo build --release -j"${JOBS:-1}"; then
  echo "✗ Сборка не удалась — откатываю код на $(git -C "$REPO" rev-parse --short "$OLD")"
  git -C "$REPO" checkout -q -B "$CUR" "$OLD"
  exit 1
fi

echo "✓ Готово: $HEAD_SHORT. Перезапустите бота:  pkill rustbot; ./start.sh"
