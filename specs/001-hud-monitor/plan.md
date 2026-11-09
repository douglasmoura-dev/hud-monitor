# Plano técnico — hud-monitor

Base: [spec.md](spec.md) · Princípios: [constitution](../../.specify/memory/constitution.md) · Decisões: [ADR 0001](../../docs/adr/0001-rust-slint.md), [ADR 0002](../../docs/adr/0002-tokens-somente-leitura.md)

## 1. Stack
| Peça | Escolha | Motivo |
|---|---|---|
| Linguagem | Rust estável (MSVC) | Sem runtime, memória previsível |
| UI | Slint com `renderer-software` + `backend-winit` | Janela sem borda/transparente, sem GPU nem WebView, poucos MB |
| Métricas | `sysinfo` | CPU por thread, RAM, discos, rede, processos |
| APIs do Windows | `windows` (PDH, DWM, Shell, IP Helper) | Temperatura, I/O de disco, acrílico, clicar através, Lixeira, ping |
| GPU NVIDIA | `nvml-wrapper` carregado em tempo de execução | Some sem erro se não houver driver NVIDIA |
| Bandeja / atalhos | `tray-icon`, `global-hotkey` | Leves, sem dependências pesadas |
| HTTP | `ureq` + rustls, em thread de fundo | Bloqueante e pequeno, sem tokio |
| Config | `serde` + `toml` em `%APPDATA%\hud-monitor\config.toml` | Legível e editável à mão |
| Erros | `thiserror` | Erros tipados (constituição VI) |
| Fontes | Rajdhani + Share Tech Mono embutidas (OFL) | Visual aprovado |

## 2. Estrutura
Regra de dependência para dentro ([ADR 0003](../../docs/adr/0003-clean-architecture.md)): `hud-app` depende de `hud-core`, nunca o contrário.
```
hud-monitor/
├─ Cargo.toml                 (workspace)
├─ crates/
│  ├─ hud-core/               domínio + casos de uso + portas; sem Windows, Slint, HTTP nem arquivo real
│  │  ├─ metrics.rs, sensors.rs   snapshot e coletor sobre portas (SystemSource, DiskIoSource, DayStore)
│  │  ├─ config.rs            Config, Profile, WindowState, MiniBarItem, Thresholds (TOML de/para texto)
│  │  ├─ alerts.rs            snapshot × thresholds → Level {Ok, Warn, Crit}
│  │  ├─ ai.rs, extras.rs     provedores das IAs, parsers, portas (CredentialsSource, UsageApi, ...)
│  │  ├─ view.rs              view models (Field, HudView, Chip, ClaudeView) prontos para a UI
│  │  ├─ dashboard.rs         caso de uso: amostra + provedores → DashboardView e escalonamento de alerta
│  │  ├─ windowing.rs         caso de uso: janelas, perfis, fixar, sair; portas WindowHost e ConfigStore
│  │  ├─ cleanup.rs, removal.rs   regras do scanner e da remoção; portas ScanFs, SystemProbe, Recycler
│  │  ├─ integration.rs, appearance.rs, position.rs, format.rs
│  │  ├─ ui_text.rs           todo texto visto pelo usuário (português)
│  │  └─ tests/dependency_rule.rs   falha se o core depender de crate de plataforma
│  └─ hud-app/                adaptadores + apresentação + composição (binário Windows)
│     ├─ sources/             implementações reais das portas (sysinfo, PDH, NVML, ping, ureq, arquivos)
│     ├─ winffi.rs            único módulo com `unsafe`
│     ├─ shell.rs, store.rs   bandeja, atalhos, visual nativo, autostart; config.toml em %APPDATA%
│     ├─ collect.rs, ai.rs    threads de coleta (1 Hz, ping 5 s) e de IAs (60 s)
│     ├─ clock.rs             relógio do sistema
│     ├─ presentation/        tipos gerados do Slint, mapeamento view model → Slint, WindowHost do Slint
│     ├─ scanner.rs           janela e threads do scanner/remoção
│     ├─ runtime.rs           timers de 1 Hz e 100 ms ligando fontes, casos de uso e telas
│     ├─ ui/                  hud.slint, minibar.slint, scanner.slint, theme.slint
│     └─ main.rs              composition root (≤ 120 linhas, sem lógica)
└─ .github/workflows/ci.yml   fmt, clippy -D warnings, test, build release (windows-latest)
```

## 3. Fluxo de dados
1. Uma thread coletora lê as fontes a 1 Hz e monta um `Snapshot` (hud-core).
2. Outra thread consulta as IAs a cada 60 s e monta `Vec<ProviderStatus>`.
3. Os dois enviam por canal para a thread de UI, que chama `slint::invoke_from_event_loop` e atualiza os modelos das duas janelas. As duas janelas leem o mesmo `Snapshot`, então nunca divergem.
4. `alerts::evaluate` roda sobre o snapshot; nível `Crit` com HUD escondido mostra a barrinha.

## 4. Fontes de dados por métrica
| Métrica | Fonte | Sem permissão / indisponível |
|---|---|---|
| CPU total, por thread, frequência | `sysinfo` | — |
| Temperatura | PDH `\Thermal Zone Information(*)\Temperature` (Kelvin → °C, maior zona) | `—` "sensor indisponível" |
| GPU NVIDIA uso/VRAM/temp | NVML | GPU desligada/ausente → `—` "GPU em repouso" |
| GPU Intel uso | PDH `\GPU Engine(*)\Utilization Percentage` (soma por adaptador) | `—` |
| RAM e top 3 processos | `sysinfo` | — |
| Disco usado/total | `sysinfo` | — |
| Disco leitura/escrita | PDH `\PhysicalDisk(_Total)\Disk Read/Write Bytes/sec` | `—` |
| Rede ↓/↑ | `sysinfo` networks, diferença por segundo, soma das interfaces ativas | — |
| Rede total do dia | acumulado em `%APPDATA%\hud-monitor\net-YYYY-MM-DD.json`, zera à meia-noite local | — |
| Ping | ICMP (`IcmpSendEcho`) para 1.1.1.1 a cada 5 s, timeout 1 s | `—` "sem resposta" |

## 5. Limites das IAs
Cada provedor implementa `trait AiProvider { fn detect(&self) -> bool; fn fetch(&self) -> Result<ProviderStatus, AiError>; }`. Os parsers de JSON ficam em `hud-core::ai` e são testados com fixtures em `crates/hud-core/tests/fixtures/`.

| IA | Detecção | Fonte primária | Plano B |
|---|---|---|---|
| Claude Pro/Max | `%USERPROFILE%\.claude\.credentials.json` | `GET https://api.anthropic.com/api/oauth/usage` com `Authorization: Bearer` e `anthropic-beta: oauth-2025-04-20` → `five_hour`, `seven_day` (`utilization`, `resets_at`). Plano vem de `claudeAiOauth.subscriptionType` | Somar tokens dos `.jsonl` em `.claude\projects` nas janelas móveis de 5 h e 7 d, marcado `~` |
| Codex | `%USERPROFILE%\.codex\auth.json` | A pesquisar na issue: endpoint de limites usado pelo próprio Codex (janelas primária 5 h e secundária semanal) | Eventos `rate_limits` nos logs locais do Codex, se existirem |
| Gemini | `%USERPROFILE%\.gemini\oauth_creds.json` | A pesquisar: cota diária da Code Assist API | Contar requisições do dia nos logs locais, marcado `~` |
| Cursor | `%APPDATA%\Cursor` | A pesquisar: uso do mês da conta | `—` |
| Copilot, OpenCode, Ollama | pastas `.copilot`, `.local\share\opencode`, processo/porta 11434 do Ollama | Só ativo/inativo | — |

**Achado de 2026-11-07:** o token do `.credentials.json` voltou 401 e o arquivo tem `expiresAt = 0`. O app **não** renova tokens (ADR 0002): renovar rotaciona o refresh token e pode deslogar o Claude Code. Com 401 o app usa o plano B e mostra no tooltip "abra o Claude Code para renovar o login".

## 6. Scanner de limpeza
| Categoria | Caminhos | Regra | Marcado |
|---|---|---|---|
| Temporários | `%TEMP%`, `C:\Windows\Temp` | arquivos com 24 h+ | sim |
| Lixeira | Shell `SHQueryRecycleBin` | esvaziar via `SHEmptyRecycleBin` (única exceção à Lixeira, já é a Lixeira) | sim |
| Cache de navegadores | perfis do Chrome/Edge `Cache`, `Code Cache` | navegador fechado | sim |
| `node_modules` antigos | busca em `%USERPROFILE%\source`, `Documents`, `Desktop` | pasta-pai sem modificação há 90+ dias | sim |
| Caches de pacotes | `%LOCALAPPDATA%\npm-cache`, `pip\cache`, `%USERPROFILE%\.nuget\packages`, `.cargo\registry\cache` | — | sim |
| Windows Update | `C:\Windows\SoftwareDistribution\Download` | exige admin → senão "precisa de admin" | não |
| Docker | `docker system df` se instalado | só mostra o comando; não apaga | não, "revisar" |
| Downloads antigos | `%USERPROFILE%\Downloads` | 180+ dias | não, "revisar" |

Remoção: `IFileOperation` com `FOFX_RECYCLEONDELETE`, em thread de fundo, com progresso. Nunca segue links simbólicos nem junções.

## 7. Memória e desempenho
- Renderizador de software do Slint, janelas pequenas; fontes subset (só Latin) para reduzir tamanho.
- `sysinfo::System` criado uma vez e atualizado só nas partes usadas (`refresh_cpu_usage`, `refresh_memory`), processos a cada 5 s.
- Scanner libera os resultados ao fechar a janela.
- CI publica o binário release; a issue de orçamento mede memória privada com HUD + barrinha por 10 min.

## 8. Temas
- `hud-core::theme`: enum `ThemeId { GamerHud, Cyberpunk, Minimal, Terminal, Light }` com serde (`gamer-hud`, `cyberpunk`, `minimal`, `terminal`, `light`) e uma paleta por tema (cores de destaque, texto, texto fraco, trilho das barras, fundo, borda, aviso, crítico, fonte de título/número). Campo `appearance.theme` no config com default `gamer-hud`, para configs antigas continuarem carregando.
- Slint: o `global Theme` vira propriedades `in-out` preenchidas pela paleta; `Minimalista` sem brilho nem cantos chanfrados, `Terminal` só Share Tech Mono.
- Bandeja: submenu "Tema" com os 5, marcado o atual; trocar salva o config.
- Sem cor livre nem temas em arquivo (decisão do usuário em 2026-11-08).

## 9. Tarefas
Ver [tasks.md](tasks.md). Cada tarefa vira uma issue com os critérios de [docs/evals.md](../../docs/evals.md).
