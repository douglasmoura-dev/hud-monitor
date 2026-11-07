# Especificação — hud-monitor

**Status:** aprovada (mockups validados com o usuário em 2026-11-07)

## Visão
Um monitor de sistema para Windows, leve, sempre por cima das outras janelas, com visual gamer HUD transparente. Mostra uso de CPU, memória, GPU, disco, rede e temperatura, os limites de uso das IAs instaladas (janelas de 5 horas, semanal, diária, mensal) e encontra arquivos que podem ser apagados para liberar espaço.

## Usuário
Desenvolvedor que usa várias IAs de código (Claude Pro, Codex, Gemini, Cursor, Copilot, OpenCode, Ollama) num notebook modesto (i5-7200U, 2 núcleos / 4 threads, Intel HD 620 + GeForce 920MX) e quer saber, sem abrir nada, quanto de cada recurso e de cada cota está sobrando.

## Cenários

### C1 — HUD completo
1. Ao abrir pela primeira vez, o HUD aparece no canto superior direito da tela principal.
2. O usuário arrasta pela barra de título para qualquer lugar; ao reabrir, volta na mesma posição.
3. Mostra, atualizando a cada 1 s: CPU total e por thread lógica, frequência, temperatura; RAM usada/total e top 3 processos; GPU e VRAM; disco C: usado/total, leitura/escrita; rede download/upload, total do dia e ping; limites das IAs; espaço limpável.

### C2 — Barrinha configurável
1. O usuário escolhe quais itens aparecem (CPU, núcleos, temperatura, RAM, GPU, disco, I/O de disco, rede, ping, cada limite de IA, relógio), em que ordem, e se a barra é horizontal ou vertical.
2. A barrinha e o HUD completo são janelas independentes: cada uma pode ficar ligada ou desligada, com posição e opacidade próprias.
3. Botão expandir na barrinha abre o HUD; botão minimizar no HUD recolhe para a barrinha.
4. Perfis nomeados (ex.: "Trabalho", "Jogo") guardam a seleção de itens e trocam por atalho.

### C3 — Aparência e controle
1. Fundo escuro semitransparente com opacidade ajustável (20–100%) e desfoque acrílico opcional.
2. Modo "clicar através": o mouse passa pela janela; segurar Ctrl permite arrastar.
3. Ícone na bandeja do Windows com resumo no tooltip e menu (mostrar HUD, mostrar barrinha, perfis, configurações, sair).
4. Atalhos globais configuráveis para mostrar/esconder cada janela.
5. Opção de iniciar com o Windows.

### C4 — Alertas
1. Cada métrica tem limites de aviso (amarelo) e crítico (vermelho) configuráveis.
2. Ao passar do crítico (ex.: Claude 5h > 90%, disco > 95%, temperatura > 85 °C), o item muda de cor e, se o HUD estiver escondido, a barrinha aparece.

### C5 — Limites das IAs
1. O app detecta sozinho quais IAs estão instaladas lendo as pastas locais delas.
2. Para cada IA com login, mostra o plano (ex.: PRO) e as janelas de uso com % e tempo até o reset:
   - Claude Pro/Max: janela de 5 horas e semanal.
   - Codex: janela de 5 horas e semanal.
   - Gemini: cota diária.
   - Cursor: uso do mês.
   - Copilot, OpenCode, Ollama: só "ativo" ou "inativo".
3. Se a fonte oficial falhar, mostra `—` com o motivo; se usar estimativa por logs locais, marca com `~`.

### C6 — Scanner de limpeza
1. Escaneia categorias: temporários do Windows, Lixeira, cache dos navegadores, `node_modules` sem uso há 90+ dias, cache de npm/pip/NuGet/cargo, cache do Windows Update, imagens e volumes Docker parados, Downloads com mais de 6 meses.
2. Mostra o tamanho de cada categoria; categorias seguras vêm marcadas, arriscadas vêm desmarcadas com o rótulo "revisar".
3. O usuário confirma e os arquivos vão para a Lixeira. O app mostra quanto foi liberado.

## Requisitos não funcionais
- Memória privada ≤ 30 MB e CPU média ≤ 1% (ver constituição).
- Funciona sem administrador; recursos que exigem admin aparecem como indisponíveis com explicação.
- Windows 10 22H2 e Windows 11.

## Fora do escopo (v1)
- Temperatura exata por núcleo via driver (fica para um modo opcional "temperatura precisa" que exige admin).
- FPS de jogos.
- macOS e Linux.
- Custo em dinheiro das APIs pagas por token.
