# Evals — critérios de aceite

Arquivo só cresce: cada issue tem um bloco aqui, e o bloco continua mesmo depois que a issue fecha. Formato: entrada concreta → resultado esperado.

## E01 — Esqueleto, janela e CI
- Input: `cargo build --release` em windows-latest → Expect: exit 0; `cargo clippy --all-targets -- -D warnings` exit 0; `cargo fmt --check` exit 0.
- Input: primeira execução sem `config.toml`, tela 1920×1080 → Expect: janela HUD sem borda, `topmost = true`, canto superior direito em x = 1920 − largura − 12, y = 12.
- Input: arrastar a barra de título para (400, 300), fechar e reabrir → Expect: janela abre em (400, 300).
- Input: posição salva (5000, 5000) e monitor atual 1920×1080 → Expect: janela reposicionada para dentro da área visível.

## E02 — Configuração e perfis (hud-core)
- Input: `Config::load` de arquivo inexistente → Expect: `Err(ConfigError::NotFound)`; quem chama cria a config inicial explícita (`Config::first_run(screen)`), nunca valores escondidos.
- Input: TOML com `opacity = 1.5` → Expect: `Err(ConfigError::Invalid { field: "opacity" })`.
- Input: salvar Config com perfis "Trabalho" e "Jogo" e recarregar → Expect: igualdade estrutural (`assert_eq!`).
- Input: `profile.switch("Jogo")` → Expect: `minibar.items == [Temp, Ping, Cpu]` conforme salvo.

## E03 — Coleta de sistema
- Input: `FakeSystem` com 4 threads a [10, 90, 50, 51] % → Expect: `snapshot.cpu.total == 50.25`, `cpu.threads.len() == 4`, `cpu.above_50 == 2`.
- Input: bytes de rede recebidos 0 → 4_404_019 em 1 s → Expect: `format::rate(net.down) == "4.2 MB/s"`.
- Input: bytes enviados 327_680 em 1 s → Expect: `"320 KB/s"`.
- Input: RAM 11.2 GiB de 16 GiB → Expect: `mem.percent == 70`, texto `"11.2/16 GB"`.
- Input: acumulado de rede às 23:59:59 = 3.1 GB, depois 00:00:01 → Expect: total do dia volta a 0 e arquivo do dia anterior fica intacto.
- Input: PDH de disco indisponível → Expect: campo `disk.io == Unavailable("contador indisponível")`, UI mostra `—`.
- Input: release binary rodando 15 s no i5-7200U → Expect: processo vivo, ~0,01 CPU-s/s, ~27 MB, `net-AAAA-MM-DD.json` criado em `%APPDATA%\hud-monitor` (verificado manualmente em T03).

## E04 — Temperatura e GPU
- Input: zonas PDH `\_TZ.TZ00 = 354.15 K`, `\_TZ.TZ01 = 340.15 K` → Expect: `thermal.cpu_c == 81`.
- Input: nenhuma zona térmica → Expect: `Unavailable("sensor indisponível")`, nunca 0 °C.
- Input: `nvml.dll` ausente → Expect: GPU NVIDIA `Unavailable("sem driver NVIDIA")`, app não quebra.
- Input: NVML responde temperatura 0 com GPU em repouso → Expect: `Unavailable("GPU em repouso")`.

## E05 — HUD completo (UI)
- Input: snapshot com 4 threads → Expect: 4 barras de núcleo, rótulo `"2C/4T"`.
- Input: thread a 85% → Expect: barra vermelha (#E24B4A); a 40% → verde (#1D9E75).
- Input: renderização do HUD → Expect: textos em Rajdhani, números em Share Tech Mono (verificado no `.slint`).
- Input: qualquer métrica `Unavailable(motivo)` → Expect: valor `"—"` e tooltip com o motivo.

## E06 — Barrinha configurável
- Input: itens `[Cpu, Ram, Net, Claude5h]`, horizontal → Expect: 4 chips nessa ordem, layout em linha.
- Input: trocar para vertical → Expect: mesmos chips em coluna, posição salva separada da do HUD.
- Input: lista de itens vazia ao salvar → Expect: `Err(ConfigError::Invalid { field: "minibar.items" })`.
- Input: clique em expandir na barrinha → Expect: HUD visível; clique em minimizar no HUD → HUD escondido e barrinha visível.

## E07 — Aparência, bandeja e atalhos
- Input: opacidade 55% → Expect: fundo `rgba(10,14,20,0.55)`.
- Input: acrílico ligado em Windows 10 22H2 → Expect: `DwmSetWindowAttribute`/`SetWindowCompositionAttribute` chamado com sucesso; se falhar, fundo sólido na mesma opacidade e aviso no log.
- Input: clicar através ligado → Expect: estilo `WS_EX_TRANSPARENT | WS_EX_LAYERED`; com Ctrl pressionado o estilo é removido até soltar.
- Input: atalho `Ctrl+Alt+M` → Expect: alterna visibilidade do HUD; atalho já registrado por outro app → aviso nas configurações, sem quebrar.
- Input: menu da bandeja "Sair" → Expect: processo encerra, config salva.
- Input: "iniciar com o Windows" ligado → Expect: valor em `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\hud-monitor` apontando para o exe; desligado → valor removido.

## E08 — Alertas
- Input: thresholds `claude_5h: warn 70, crit 90`, uso 72 → Expect: `Level::Warn`; uso 91 → `Level::Crit`.
- Input: `Level::Crit` com HUD e barrinha escondidos → Expect: barrinha fica visível.
- Input: temperatura `Unavailable` → Expect: nenhum alerta disparado.

## E09 — Claude Pro/Max
- Input: fixture `claude_usage_ok.json` `{"five_hour":{"utilization":72,"resets_at":"2026-11-07T17:44:00Z"},"seven_day":{"utilization":41,"resets_at":"2026-11-10T12:00:00Z"}}` com agora = 2026-11-07T15:56:00Z → Expect: `five_hour.percent == 72`, `reset_in == "1h48"`; `seven_day.percent == 41`, `reset_in == "2d"`.
- Input: `.credentials.json` com `subscriptionType: "pro"` → Expect: etiqueta `"PRO"`; `"max"` → `"MAX"`.
- Input: resposta 401 → Expect: plano B por logs locais; valores com prefixo `~` e tooltip "abra o Claude Code para renovar o login".
- Input: 401 e nenhum `.jsonl` nas últimas 5 h → Expect: `~0%` na janela de 5 h.
- Input: qualquer execução → Expect: token não aparece em log, tela nem arquivo do app (teste procura a string do token fake na saída do log); arquivo de credenciais com hash igual antes e depois.

## E10 — Codex
- Input: fixture da resposta de limites com primária 23% (reset em 11400 s) e secundária 58% (reset em 432000 s) → Expect: `5h 23% 3h10`, `semana 58% 5d`.
- Input: sem `auth.json` → Expect: Codex não aparece na lista.
- Input: 401 → Expect: `—` com "abra o Codex para renovar o login".

## E11 — Gemini, Cursor e presença
- Input: fixture de cota Gemini 120 de 1000 → Expect: `dia 120/1000`, 12%.
- Input: fixture de uso Cursor 64% com reset em 18 dias → Expect: `mês 64% 18d`.
- Input: pasta `.copilot` existe → Expect: Copilot listado como presente; Ollama respondendo em `127.0.0.1:11434/api/tags` → "ativo"; sem resposta → "inativo".

## E12 — Scanner (somente leitura)
- Input: árvore fake com `proj-a/node_modules` (pai modificado há 120 dias, 500 MB) e `proj-b/node_modules` (há 10 dias) → Expect: só `proj-a` listado, 500 MB.
- Input: `Downloads/old.zip` de 200 dias → Expect: categoria Downloads desmarcada com `review = true`.
- Input: junção/link simbólico dentro de `%TEMP%` apontando para `C:\Users` → Expect: não seguido, tamanho não somado.
- Input: Windows Update sem admin → Expect: categoria com `Unavailable("precisa de admin")`.

## E13 — Limpeza para a Lixeira
- Input: confirmar categorias Temp (3 arquivos) e node_modules (1 pasta) num diretório de teste → Expect: itens somem do disco e aparecem na Lixeira (`SHQueryRecycleBin` aumenta); nenhuma chamada de exclusão permanente no código.
- Input: arquivo travado por outro processo → Expect: item pulado, listado em "não foi possível mover", resto continua.
- Input: total liberado → Expect: mensagem com a soma dos itens movidos, não dos selecionados.

## E14 — Orçamento de memória e CPU
- Input: build release, HUD + barrinha visíveis, 10 min a 1 Hz, i5-7200U → Expect: memória privada máxima ≤ 30 MB e CPU média ≤ 1% (medido por script PowerShell em `scripts/measure.ps1`).
- Input: abrir e fechar o scanner 5 vezes → Expect: memória privada volta a ≤ 30 MB em 10 s.

## Verificações adicionais — T02 (E02)
- Input: TOML com `thresholds.cpu_percent` warn >= crit → Expect: `Err(ConfigError::Invalid { field: "thresholds.cpu_percent" })`.
- Input: dois perfis com o mesmo nome → Expect: `Err(ConfigError::Invalid { field: "profiles" })`.
- Input: `switch_profile("Nope")` → Expect: `Err(ConfigError::Invalid { field: "profile" })`.
- Input: build release iniciado sem `config.toml` → Expect: HUD abre (config de primeira execução em memória) e processo fica vivo.

## Verificações adicionais — T04 (E04)
- Input: driver NVIDIA 471.11 (GeForce 920MX) sem `nvmlDeviceGetMemoryInfo_v2` → Expect: VRAM `Unavailable("VRAM indisponível neste driver")`, temperatura e uso da GPU NVIDIA continuam disponíveis.
- Input: zonas térmicas e GPU Intel (HD 620) reais, build release → Expect: janela mostra `TEMP` em °C e uso da Intel em 
## Verificações adicionais — T05 (E05)
- Input: `SystemDrive` indisponível/disco não encontrado (fake com drive `D:`) → Expect: `usage == Unavailable("disco D: indisponível")`.
- Input: `FileDayStore::save` duas vezes na mesma data → Expect: arquivo contém o último valor e nenhum `.tmp` sobra no diretório (escrita atômica por rename).
- Input: contagem de núcleos físicos indisponível → Expect: rótulo `"—/4T · …"` com tooltip do motivo.
- Input: build release rodando 30 s em i5-7200U, captura em `docs/screenshots/hud.png` → Expect: HUD 320x400 com fontes Rajdhani/Share Tech Mono, memória privada 14,3 MB (≤ 30 MB).

## E06 — verificações executadas (T06)
- Executei `target\release\hud-app.exe` com config em `%APPDATA%` temporário (barrinha visível, itens `[Cpu, Ram, Net, Claude5h]`, horizontal) por 30 s: 4 chips nessa ordem em linha, `5H` mostra `—` ("provedor ainda não implementado"); captura em `docs/screenshots/minibar.png`. Memória privada: 13,8 MB (orçamento 30 MB).
- Botões expandir/fechar na barrinha e minimizar no HUD: cobertos por testes do `.slint` e por `save_minibar_state`; o clique real não foi automatizado.

## E07 — verificações executadas (T07)
- Executei `target\release\hud-app.exe` com `%APPDATA%` temporário e config com HUD `opacity = 0.55`, `acrylic = true`, `click_through = true`, `autostart = true`: nenhum aviso no stderr (acrílico via `SetWindowCompositionAttribute` aplicado; captura em `docs/screenshots/hud-acrylic.png`), estilo estendido `0xC0138` (`WS_EX_TRANSPARENT` e `WS_EX_LAYERED` presentes) e `HKCU\...\Run\hud-monitor` apontando para o exe entre aspas. Memória privada: 14,3 MB (orçamento 30 MB).
- Ctrl simulado por `keybd_event`: segurado → `WS_EX_TRANSPARENT` removido; solto → volta. Config com `autostart = false` → valor `hud-monitor` removido do `Run` (confirmado por leitura do registro; nenhum valor deixado na máquina).
- Atalho `Ctrl+Alt+M` simulado → config salva com barrinha visível e HUD oculto. Tooltip, menu da bandeja (perfis, sair) e conflito real de atalho: só cobertos por testes com fakes, clique real na bandeja não automatizado.
- Pino do HUD: alterna clicar através (Ctrl suspende); coberto por teste do `.slint`.

## Verificações T08 (issue #9)
- `cargo test`: `alerts::tests` cobre 72 → Warn, 91 → Crit, temperatura `Unavailable` → sem alerta, limiar por métrica, snapshot degradado sem alerta; `config::tests` cobre Crit com HUD e barrinha escondidos → barrinha visível; `view::tests` cobre cores âmbar `#EF9F27`/vermelho `#E24B4A` no HUD e nos chips.
- `target\release\hud-app.exe` iniciou e rodou 6 s sem falhar (cor sob alerta real não foi forçada na tela).
