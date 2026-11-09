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

## Verificações T09 (issue #10)
- `cargo test`: `ai::tests` cobre a fixture `claude_usage_ok.json` (72% / `1h48`, 41% / `2d`), etiquetas `PRO`/`MAX`, 401 → estimativa por logs locais com "abra o Claude Code para renovar o login", sem tokens → `~0%`, e que o token fake não aparece em nenhuma saída `Debug`/`Display`; `sources::claude::tests` percorre `.jsonl` reais em pasta temporária (só arquivos dos últimos 7 dias) e confere que o hash do arquivo de credenciais é igual antes e depois.
- `target\release\hud-app.exe` na máquina real (`%APPDATA%` temporário, `USERPROFILE` real): o endpoint voltou 401 (`expiresAt = 0`), então o HUD mostrou a etiqueta `PRO` e o plano B: `5H ~5.3M tok` e `SEMANA ~28.8M tok`, sem percentual nem reset (limite por plano não é público). stderr vazio, hash do `.credentials.json` idêntico, memória privada 15,2 MB; captura em `docs/screenshots/hud-ai.png`.
- Plano B mostra contagem de tokens em vez de percentual inventado; `~0%` só quando a contagem é zero. Alerta `claude_5h` (70/90) só dispara com leitura oficial. O caminho oficial (200) só foi exercitado com fixture e fakes, não contra a API real.

## Verificações T10 (issue #11)
- Endpoint: o Codex CLI lê os limites com `GET https://chatgpt.com/backend-api/wham/usage` (cabeçalhos `Authorization: Bearer` e `chatgpt-account-id`; janelas `primary_window` e `secondary_window` com `used_percent` e `reset_after_seconds`), uma leitura que não consome cota. Plano B (eventos `rate_limits` nos logs locais) não foi implementado: o endpoint existe, e nesta máquina os logs de sessão são `.jsonl.zst`.
- `cargo test`: `ai::tests` cobre a fixture `codex_usage_ok.json` (23% / `3h10`, 58% / `5d`), `auth.json` ausente → `Codex não encontrado` (não listado), 401 → `—` com "abra o Codex para renovar o login", e que o token fake não aparece em nenhuma saída; `sources::claude::tests` confere que o hash do `auth.json` é igual antes e depois; `view::tests` cobre a linha do HUD, `codex_listed` e os chips `CX5H`/`CX7D`.
- `target\release\hud-app.exe` na máquina real (`%APPDATA%` temporário, `USERPROFILE` real, Codex instalado): GET real devolveu 200 com 0% / 0% (resets 18000 s e 604800 s); o HUD mostrou `CODEX`, `5H 0% 4h59` e `SEMANA 0% 6d`. stderr vazio, hash do `auth.json` idêntico, memória privada 15,1 MB (orçamento 30 MB). A captura não foi versionada porque o HUD é translúcido e mostrava janelas do desktop ao fundo.
- A altura do HUD acompanha `codex-listed` (566 px com Codex, 500 px sem); o redimensionamento ao vivo da janela só foi observado com Codex listado.

## Verificações T11 (issue #12)
- Gemini: o gemini-cli consulta a cota com `POST cloudcode-pa.googleapis.com/v1internal:retrieveUserQuota` (precisa de `loadCodeAssist` antes e de token OAuth renovado), devolvendo fração por modelo, não contagem diária. Não é um GET somente leitura e não podemos renovar token (ADR 0002), então foi usado o plano B: respostas únicas (por `id`, o log reescreve a linha ao anexar tool calls) com `"type":"gemini"` em `~/.gemini/tmp/**/session-*.jsonl` das últimas 24 h, marcado `~` (`~dia 120/1000`, 12%). O limite 1000 é o gratuito documentado, não lido da conta.
- Cursor: o token fica em `state.vscdb` (SQLite); ler exigiria uma dependência nova só para uma linha, então o uso fica `—` com "uso do Cursor indisponível". O parser do JSON de resumo de uso (`totalPercentUsed`, `billingCycleEnd`) está coberto por fixture (`mês 64% 18d`) mas o formato é de memória, não verificado contra a API real.
- Presença: `.copilot` e `.local\share\opencode` listam "presente"; Ollama via GET `127.0.0.1:11434/api/tags` com 1 s, "ativo"/"inativo" (testado com servidor TCP local).
- `cargo test`: `extras::tests` (core) e `sources::extras::tests` (app, incluindo hash do arquivo de credenciais inalterado e token fake ausente). Executável release iniciado 32 s: memória privada 15,5 MB; binário 9 611 776 → 9 678 336 bytes (+66 KB, nenhuma dependência nova). Captura não versionada (HUD translúcido).

## Verificações T12 (issue #13)
- `cargo test`: `cleanup::tests` (core, com `FakeFs`) cobre `proj-a/node_modules` (pai há 120 dias, 500 MB) listado e `proj-b` (10 dias) ignorado, `Downloads/old.zip` de 200 dias com `checked = false` e `review = true`, link dentro de `%TEMP%` apontando para `C:\Users` não seguido nem somado, Windows Update sem admin com `Unavailable("precisa de admin")`, regra de 24 h dos temporários, Docker só listado quando instalado (e nunca medido), pastas ocultas fora da busca de projetos, total dos marcados. `sources::cleanup::tests` (app) usa pasta temporária real: uma junção (`mklink /J`) apontando para fora da árvore escaneada não é seguida (só os 100 bytes próprios são somados) e os arquivos de fora ficam intactos.
- `target\release\hud-app.exe --scanner` na máquina real (`APPDATA` temporário), aberto 25 s e 150 s: a janela "Limpeza de disco" abriu, o HUD continuou respondendo, as categorias apareceram uma a uma (Temporários `≥ 1.8 GB` por pastas travadas, Lixeira `0 B`, Cache de navegadores `1.4 GB`, `node_modules antigos` `≥ 9.0 GB`) e as seguras vieram marcadas, com "Selecionado" somando só as marcadas. stderr vazio; memória privada 18,0-18,6 MB com HUD + scanner (orçamento 30 MB).
- Escaneamento completo nesta máquina passa de 150 s (muitas pastas em Documents/source); a janela não trava. Não foi observado o fim do scan na tela, então os rótulos "revisar" e o estado de Windows Update sem admin só foram verificados por teste unitário, não na janela real. Nada é apagado: o scanner só lista diretórios e lê metadados.

## E15 — Temas visuais
- Input: `config.toml` sem `appearance.theme` → Expect: carrega com tema `gamer-hud` e as cores atuais (`#5DCAA5` destaque).
- Input: `theme = "cyberpunk"` / `"minimal"` / `"terminal"` / `"light"` → Expect: HUD, barrinha e scanner usam a paleta do tema; `theme = "xyz"` → config inválida com erro que nomeia `appearance.theme`.
- Input: bandeja → Tema → Terminal retrô com o HUD aberto → Expect: as cores mudam em ≤ 1 s sem reiniciar e o config salvo tem `theme = "terminal"`.
- Input: cada tema com uma métrica em Warn e outra em Crit → Expect: contraste de texto de alerta sobre o fundo do tema ≥ 3:1 (teste sobre a paleta).
- Input: build release com tema trocado 10 vezes → Expect: memória privada continua ≤ 30 MB.

## Verificações T13 (issue #14)
- `cargo test`: `sources::cleanup::tests` (app) usa o backend real (`IFileOperation`, `FOFX_RECYCLEONDELETE`) só com arquivos criados em pasta temporária: 3 arquivos `.tmp` e 1 pasta `node_modules` somem do disco e `SHQueryRecycleBin` sobe em pelo menos 4 itens; um arquivo aberto com `share_mode(0)` é pulado e reportado enquanto o outro segue para a Lixeira; uma junção (`mklink /J`) não é seguida (alvo e junção intactos) e um item inexistente é reportado.
- `cargo test`: `removal::tests` (core, com fakes) cobre o plano refeito na hora da remoção (temporário de 0 dias, `node_modules` com pai de 10 dias e Downloads de 10 dias ficam de fora), links nunca listados, Docker e Windows Update sem admin nunca planejados, item travado pulado e listado com o resto continuando, total liberado = soma dos movidos (100 B de 1000 B selecionados) e Lixeira esvaziada só pelo `Recycler`; `scanner::tests` cobre a confirmação (categorias e total) sem iniciar nada, cancelar, progresso e tela de resultado. Testes de código garantem que não há `remove_file`/`remove_dir`/`DeleteFile` nem `FOF_NOCONFIRMATION` fora dos módulos de teste.
- Não verificado: `SHEmptyRecycleBin` (esvaziaria a Lixeira real da máquina, então só é coberto pelo fake) e a janela real com clique em "Confirmar" (nenhuma categoria real foi removida nesta máquina).

## E16 — Código em inglês, clean code e clean architecture
- Input: `cargo test` antes e depois → Expect: mesma quantidade ou mais testes, todos passando; nenhum teste removido sem substituto equivalente.
- Input: `cargo tree -p hud-core` → Expect: nenhuma dependência de `slint`, `windows`, `ureq`, `tray-icon`, `global-hotkey`, `nvml-wrapper`.
- Input: `grep` por palavras em português em identificadores, comentários e `#[error]` de `crates/**/*.rs` → Expect: zero, exceto no módulo de strings da UI e em fixtures.
- Input: `crates/hud-app/src/main.rs` → Expect: ≤ 120 linhas, só composição.
- Input: build release executado com o mesmo `config.toml` → Expect: HUD, barrinha, bandeja, IAs e scanner iguais ao antes (mesmos textos em português na tela), memória privada ≤ 30 MB.

## Verificações T16 (issue #31)
- `cargo test` antes: 55 (hud-app) + 150 (hud-core) = 205 testes; depois: 62 (hud-app) + 175 (hud-core) + 4 (`tests/dependency_rule.rs`) = 241, todos passando, nenhum removido. Os testes de `main.rs` sobre os arquivos `.slint` foram para `presentation/mod.rs` com as mesmas asserções. Onde um teste comparava um texto de tela em português, ele agora compara com a constante/função de `ui_text`, e os testes de `ui_text` fixam as palavras em português; fixtures opacas (ex.: `sem login`) viraram inglês.
- Novos: `dependency_rule` (o core não depende de crate de plataforma nem usa `std::fs`/`std::env`/`std::net`/`std::process` no código de produção), `dashboard::tests` e `windowing::tests` (casos de uso extraídos do `main.rs`, com fakes de `WindowHost` e `ConfigStore`), `ui_text::tests`, mapeamento Slint, `store` e `clock`.
- `cargo tree -p hud-core`: só `serde`, `serde_json`, `thiserror` e `toml`; zero ocorrências de `slint`, `windows`, `ureq`, `tray-icon`, `global-hotkey`, `nvml-wrapper`.
- `crates/hud-app/src/main.rs`: 603 → 84 linhas, só composição. Nenhum `unwrap`/`expect` fora de testes; `unsafe` só em `winffi.rs`. Os testes de código que proíbem `remove_file`/`DeleteFile`/`FOF_NOCONFIRMATION` continuam e passam.
- Português em `crates/**/*.rs`: nenhuma palavra acentuada fora de `ui_text.rs`; identificadores, comentários e `#[error]` em inglês. Sobram só asserções sobre o texto dos `.slint` (`presentation/mod.rs`), que são da camada de apresentação.
- Build release (`cargo build --release`, `APPDATA` temporário, config padrão): o HUD abriu com `CPU`, `RAM`, `GPU`, `DISCO`, `REDE` (`hoje 105 KB`), `LIMITES IA` com `CLAUDE` (estimativa `~` marcada), `CODEX` (`0%`), `GEMINI` (`dia 0/1000` estimado) e `OUTROS`; com um `config.toml` de barrinha (`Cpu`, `Ram`, `Disk`, `Claude5h`, `Codex5h`) a barrinha mostrou `CPU`, `RAM`, `DISCO`, `5H`, `CX5H` com a cor de alerta; a janela oculta da bandeja existia. `--scanner` abriu "Limpeza de disco" com `Escaneando...`, `Temporários` (`algumas pastas não puderam ser lidas...`), `Lixeira` (`30 itens`), `Cache de navegadores` e o botão "Mover para a Lixeira"; nada foi confirmado nem movido. O `config.toml` gravado tem as mesmas chaves de antes.
- Memória privada: 15,7 MB (HUD), 14,9 MB (barrinha), 18,0 MB (HUD + scanner após 40 s); orçamento 30 MB. Binário 9 824 768 bytes.
- Não verificado: menu da bandeja clicado (só a existência da janela da bandeja) e comparação lado a lado com um build anterior (não havia executável antigo disponível; a comparação é pelos textos acima e pelos testes). As capturas não foram versionadas (HUD translúcido mostra o desktop ao fundo).

## Verificações T14 (issue #15)
- Medição: `scripts/measure.ps1` (release, `APPDATA` temporário, HUD visível, 15 s de aquecimento, amostras de 1 s por 600 s, i5-7200U, 4 threads lógicas). O app mostra o HUD ou a barrinha, nunca os dois ao mesmo tempo; a medição usa o HUD (o mais pesado). A constituição não diz "1% de quê"; o script usa a leitura mais rígida como limite (% de UM núcleo = tempo de CPU / tempo real) e também imprime % da máquina inteira (dividido por 4).
- Antes (main em 254b9dc): memória privada máxima 18,6 MB; CPU média 2,86% de um núcleo (0,72% da máquina) — acima do orçamento na leitura de um núcleo.
- Depois: memória privada máxima 16,1 MB; CPU média 0,99% de um núcleo (0,25% da máquina) — dentro do orçamento, mas rente ao limite na leitura de um núcleo (a medição varia algumas décimas entre execuções, com outros processos pesados na máquina).
- Onde ia a CPU (tempo de CPU por thread, medido com a máquina em uso normal): enumeração de interfaces de rede do `sysinfo` a cada segundo (`GetIfTable2` + `GetAdaptersAddresses`), o contador PDH `\GPU Engine(*)` lendo todas as instâncias de todos os processos, a varredura completa dos transcritos do Claude (2 GB de `.jsonl`) a cada 60 s, a atualização de processos a cada 5 s, o timer de 100 ms da UI e a atribuição de todas as propriedades Slint (inclusive modelos novos) a cada segundo mesmo sem mudança.
- O que mudou: rede lida com `GetIfEntry2` sobre a lista de interfaces físicas (refeita a cada 30 s); contador de GPU Intel limitado ao adaptador (`\GPU Engine(*luid_...*)`); transcritos lidos de forma incremental (só os bytes acrescentados, só linhas completas); top 3 processos a cada 10 s; timer de bandeja/atalhos a 200 ms; propriedades Slint só são atribuídas quando a parte correspondente da visão mudou. A atualização das métricas continua a 1 Hz.
- Input: scanner aberto e fechado 5 vezes (`measure.ps1 -Scanner`, flag `--scanner-cycles 5` do app, uma abertura/fechamento por segundo) → memória privada 16,9 MB logo após o último fechamento (≤ 30 MB, dentro de 10 s).
- Testes novos: `cycle_step` (alternância abrir/fechar), `changes` (só o que mudou é reaplicado), scanner de transcritos incremental (linha incompleta, arquivo encolhido, arquivo removido), caminho do contador de GPU, `NetCounters` na máquina real, `parse_usage_line`/`TokenSums::add` via `tally_line`.

## Verificações T15 (issue #29)
- `cargo test`: `config::theme_tests` (core) confirma que um `config.toml` sem `[appearance]` carrega como `gamer-hud` e é igual ao config padrão, que os 5 ids (`gamer-hud`, `cyberpunk`, `minimal`, `terminal`, `light`) sobrevivem a salvar e carregar, e que `theme = "xyz"` falha com mensagem contendo `appearance.theme`. `theme::tests` confirma que a paleta padrão mantém `#5DCAA5` de destaque e `#0A0E14` de fundo, e que aviso e crítico têm contraste ≥ 3:1 sobre o fundo de cada um dos 5 temas.
- `cargo test`: `windowing::tests::switching_theme_repaints_the_windows_and_saves_the_choice` (core, com fakes) confirma que trocar o tema pede a repintura das janelas e grava `theme` no config; `presentation::tests` confirma que HUD, barrinha e scanner desenham com `Theme.*` e não têm mais cor fixa de fundo.
- Release, `scripts/measure.ps1 -Themes` (novo, usa `--theme-cycles 10`, que troca o tema pelo mesmo caminho da bandeja): memória privada 15,9 MB depois de 10 trocas (limite 30 MB).
- Release, `scripts/measure.ps1 -DurationSeconds 240` (HUD aberto, config antigo sem `[appearance]`): memória máxima 15,6 MB, CPU média 0,85% de um núcleo (0,21% da máquina), dentro do orçamento de 1%.
- Release: `target\release\hud-app.exe` com `theme = "terminal"` e com `theme = "light"` sobe e continua rodando; com `theme = "xyz"` encerra com código 1.
- Não verificado: o clique real em Bandeja → Tema (o menu da bandeja não foi acionado com o mouse; o caminho de código é o mesmo do `--theme-cycles`) e a aparência das paletas em tela (nenhuma captura foi feita nem versionada).

## E17 — Temas v2 (identidade própria)
- Input: tema `cyberpunk` → Expect: títulos e números em Orbitron, faixa de título amarela (`#FCEE0A`) com texto escuro, cantos cortados na diagonal, barras em blocos segmentados.
- Input: tema `minimal` → Expect: Inter, rótulos em minúsculas, nenhuma barra de progresso visível, números maiores que no `gamer-hud`.
- Input: tema `terminal` → Expect: VT323 em todos os textos, barras como texto ASCII (`[####....]` com o número de `#` proporcional ao %), título em forma de prompt, linhas de varredura.
- Input: tema `light` → Expect: fundo claro, métricas principais em cartões com ponto de status colorido pela severidade.
- Input: teste sobre os estilos → Expect: nenhum par de temas tem a mesma combinação de fonte de número + estilo de barra + forma de canto.
- Input: build release, tema trocado 10 vezes e 10 min parado → Expect: memória privada ≤ 30 MB e CPU média ≤ 1% de um núcleo; exe com as fontes novas e os arquivos OFL no repositório.

## E18 — Redimensionar por escala
- Input: arrastar a alça do canto do HUD +50% na diagonal → Expect: HUD com 1,5× a largura e a altura base, fontes e barras proporcionais; ao soltar, `config.toml` tem `scale = 1.5` na seção do HUD.
- Input: `scale = 5.0` ou `0.1` no config → Expect: carrega preso a 2.0 ou 0.6, sem erro.
- Input: config sem `scale` → Expect: escala 1.0 (configs antigas continuam carregando).
- Input: escala 2.0 com o HUD perto da borda da tela → Expect: a janela é puxada para dentro da tela usando o tamanho escalado.
- Input: barrinha redimensionada para 0.8 → Expect: só a barrinha muda; o HUD mantém a própria escala.
- Input: modo "clicar através" ligado sem Ctrl → Expect: a alça não responde; com Ctrl segurado, redimensiona.
