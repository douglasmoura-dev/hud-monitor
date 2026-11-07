# Constituição — hud-monitor

Princípios que valem para toda mudança no projeto. Um PR que viole um deles não entra.

## I. Leveza acima de tudo
- Build release rodando com HUD completo + barrinha visíveis: **memória privada ≤ 30 MB** e **CPU média ≤ 1%** atualizando a 1 Hz (medido num i5-7200U).
- Proibido embutir navegador/WebView, runtime .NET, Electron ou qualquer processo auxiliar residente.
- Coleta de dados nunca bloqueia a thread de UI; chamadas de rede das IAs rodam no máximo a cada 60 s.

## II. Sem valores inventados
- Nenhum default ou fallback que pareça dado real. Se um sensor ou API não responde, a UI mostra `—` com o motivo no tooltip ("sem permissão", "sem login", "sensor indisponível").
- Nunca exibir número estimado sem marcar como estimado (ex.: `~41%`).

## III. Credenciais são intocáveis
- Tokens de IA são lidos dos arquivos locais das próprias ferramentas e usados só contra o serviço dono do token.
- Nunca logar, exibir, copiar para outro arquivo ou enviar tokens para terceiros.

## IV. Apagar é sempre reversível
- O scanner de limpeza só move para a Lixeira do Windows; nunca exclusão permanente.
- Itens arriscados (Downloads, volumes Docker) vêm desmarcados e marcados como "revisar".
- Nada é removido sem confirmação explícita mostrando o total.

## V. Testes primeiro
- TDD: teste falhando antes do código. Todo módulo novo ou alterado tem testes.
- Toda fonte externa (sensores, APIs, sistema de arquivos) fica atrás de um trait, com fake nos testes.
- CI roda `cargo fmt --check`, `cargo clippy -- -D warnings` e `cargo test` em Windows; PR só entra verde.

## VI. Código simples
- Sem `unsafe` fora do módulo de FFI do Windows, e cada bloco com comentário de justificativa.
- Sem `unwrap()`/`expect()` fora de testes; erros tipados com `thiserror`.
- Funções curtas, nomes claros, um PR por issue.
