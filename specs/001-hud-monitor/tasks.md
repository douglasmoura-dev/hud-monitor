# Tarefas — hud-monitor

Cada tarefa vira uma issue com o rótulo `spec-driven`. Critérios em [docs/evals.md](../../docs/evals.md). A ordem respeita as dependências; tarefas no mesmo nível podem rodar em paralelo.

| # | Tarefa | Evals | Depende de |
|---|---|---|---|
| T01 | Esqueleto do workspace, janela HUD sem borda/topmost/arrastável, CI no GitHub Actions | E01 | — |
| T02 | Modelo de configuração, perfis e persistência (hud-core) | E02 | T01 |
| T03 | Coleta de sistema: CPU, RAM, disco, I/O, rede, total diário, ping | E03 | T01 |
| T04 | Temperatura (PDH) e GPU (NVML + PDH Intel) | E04 | T03 |
| T05 | HUD completo ligado aos dados, tema e fontes | E05 | T02, T03 |
| T06 | Barrinha configurável e alternância com o HUD | E06 | T05 |
| T07 | Opacidade, acrílico, clicar através, bandeja, atalhos, iniciar com Windows | E07 | T06 |
| T08 | Alertas por limite | E08 | T05 |
| T09 | Provedor Claude Pro/Max | E09 | T02 |
| T10 | Provedor Codex | E10 | T09 |
| T11 | Provedores Gemini e Cursor, presença de Copilot/OpenCode/Ollama | E11 | T09 |
| T12 | Scanner de limpeza (somente leitura) e tela do scanner | E12 | T05 |
| T13 | Mover para a Lixeira com confirmação | E13 | T12 |
| T14 | Orçamento de memória e CPU + script de medição | E14 | T07, T11, T13 |
| T15 | Temas visuais (5 temas prontos, troca ao vivo) | E15 | T14 |
| T16 | Código em inglês, clean code e clean architecture (refatoração sem mudar o comportamento) | E16 | T13 |
| T17 | Temas v2: identidade própria por tema (fontes, cantos, barras, enfeites, layout) | E17 | T15 |
| T18 | Redimensionar HUD e barrinha por escala, salvo por janela | E18 | T17 |
