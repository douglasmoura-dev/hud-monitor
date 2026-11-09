# ADR 0003 — Clean architecture e código em inglês

**Status:** aceita · 2026-11-08

## Contexto
O código nasceu com a lógica de apresentação, o laço de timers e a escolha de qual janela mostrar misturados no `main.rs` (cerca de 600 linhas), e com textos em português espalhados por vários módulos do `hud-core`. Isso dificulta testar e trocar partes (T14 mede o app, T15 troca temas).

## Decisão
- **Regra de dependência para dentro.** `hud-core` é domínio + casos de uso + portas (traits), sem `slint`, `windows`, `ureq`, `tray-icon`, `global-hotkey`, `nvml-wrapper` nem acesso direto a arquivo, rede ou ambiente. Um teste (`crates/hud-core/tests/dependency_rule.rs`) lê o `Cargo.toml` e o código do core e falha se isso mudar.
- **`hud-app` = adaptadores + apresentação + composição.** `sources/`, `winffi.rs`, `shell.rs`, `store.rs` e `collect.rs`/`ai.rs` implementam as portas; `presentation/` mapeia view models do core para o Slint; `runtime.rs` liga os timers; `main.rs` só monta o grafo (≤ 120 linhas).
- **Casos de uso no core, com fakes nos testes:** `dashboard` (monta tudo o que as telas mostram e decide quando um alerta crítico escala) e `windowing` (qual janela aparece, perfis, fixar, sair), este atrás das portas `WindowHost` e `ConfigStore`.
- **Idioma.** Identificadores, comentários, mensagens de erro internas (`#[error]`), nomes de testes, commits e PRs em inglês. Todo texto que o usuário vê (HUD, barrinha, bandeja, scanner, tooltips, motivos de `Unavailable`) continua em português e fica num único módulo, `hud_core::ui_text`. Docs (spec, plano, evals, ADRs, README) seguem em português.
- Sem comportamento novo: o `config.toml` existente continua valendo e as regras de segurança (tokens somente leitura, Lixeira, sem seguir links) não mudam.

## Consequências
- Trocar um texto da tela mexe em um arquivo; trocar o Slint por outra UI não toca o core.
- Os motivos mostrados na tela já não vêm do `Display` dos erros: `ui_text::ai_error_reason` converte o erro tipado em português.
- Ler e gravar o `config.toml` é do adaptador `store.rs`; o core só converte de/para TOML.
