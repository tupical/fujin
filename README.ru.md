# Fujin 風神 — actions-слой Meisei

> **Meisei** 明晰 («ясность») — открытый конвейер, который проводит сырой замысел
> через понимание → решение → план → действие к готовому результату.

[![Meisei](https://img.shields.io/badge/meisei-明晰-1f2937.svg)](https://meisei.ru)
[![License: Apache-2.0 WITH Commons-Clause](https://img.shields.io/badge/license-Apache--2.0%20WITH%20Commons--Clause-blue.svg)](LICENSE)

<sub>
torii · satori · enma · yatagarasu · <b>fujin</b> · daruma
&nbsp;—&nbsp; intake · осмысление · решения · планирование · <b>действия</b> · исполнение (терминальный слой)
</sub>

## Что это

Fujin — **actions**-слой конвейера MeiSei: граница зрелости между
обдумыванием и исполнением. Его операция `pack_ai` собирает типизированный
`ActionPacket` (work order, шаги исполнения, гейты, целевые файлы, требуемые
документы, проекты/задачи handoff), а детерминированная проверка зрелости
(`maturity::assess`, уровни строгости и minimality-проверки M2–M6/W1–W2)
решает, созрел ли пакет для передачи — только зрелый пакет может перейти в
daruma как задачи/планы. Невалидные аргументы модели получают одну schema-aware
попытку исправления; повторная ошибка закрывает проход. Доменные примитивы не зависят от хранилища; action
packet'ы персистит сервер. Крейт не зависит от daruma и соседних слоёв;
конкретные daruma-адаптеры живут внутри host.

## Структура репозитория

- `src/` — библиотека `fujin`: типы `ActionPacket`/handoff, `pack_ai`,
  детерминированная оценка зрелости, minimality-политика, типы ошибок.
- `server/` — `fujin-server`, тонкая независимо развёртываемая HTTP/MCP-обёртка
  над библиотекой (axum/tokio-каркас — из [`layer-kit`](../layer-kit)).
- `deploy/` — release-`build.sh` (прошивает git SHA в `/healthz`) и systemd user unit.

## Сборка и запуск

```sh
cargo run -p fujin-server
# GET  /healthz   — открытая проба живости/версии
# POST /v1/mcp    — MCP-поверхность под платформенным токеном:
#                   fujin.pack, fujin.assess, fujin.list /
#                   fujin.list_packets, fujin.get / fujin.get_packet
```

Для продовых сборок используйте `deploy/build.sh`, чтобы `/healthz` отдавал
реальный git SHA, а не `"dev"`.

## Конфигурация (env)

| Переменная | По умолчанию | Назначение |
| --- | --- | --- |
| `FUJIN_PORT` | `8094` | HTTP-порт |
| `FUJIN_PLATFORM_SECRET` | не задан | HMAC-ключ; если не задан, `/v1/mcp` закрыт |
| `FUJIN_VERSION` | версия крейта | Версия, отдаваемая `/healthz` |
| `FUJIN_DB` | `./fujin.db` | Путь к SQLite-хранилищу (`layer_kit::store::Store`) |
| `OPENAI_API_KEY` | не задан | Опциональный AI-fallback провайдер для `fujin.pack`; без ключа метод отвечает 503 `ai_not_configured` |
| `OPENAI_BASE_URL` | `https://api.openai.com/v1` | Базовый URL OpenAI-совместимого API |
| `OPENAI_MODEL` | `gpt-4.1` | Модель, используемая AI-fallback'ом |

## Документация

Канон конвейера и контракты слоёв: https://meisei.ru/docs

## Лицензия

Apache-2.0 WITH Commons-Clause — см. [LICENSE](LICENSE) и
[LICENSE.commons-clause.md](LICENSE.commons-clause.md).
