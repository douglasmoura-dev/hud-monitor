# ADR 0002 — Tokens das IAs são somente leitura

**Status:** aceita · 2026-11-07

## Contexto
Para mostrar os limites de 5 horas e semanais, o app usa os logins que as próprias ferramentas (Claude Code, Codex, Gemini CLI, Cursor) já guardam na máquina. Esses tokens expiram. Renovar um token OAuth normalmente rotaciona o refresh token: o arquivo da ferramenta passa a ter um token inválido e o usuário é deslogado.

## Decisão
- O app só **lê** os arquivos de credencial; nunca escreve neles e nunca chama endpoints de renovação.
- Token expirado ou 401 → provedor mostra `—` com "abra o <ferramenta> para renovar o login", ou usa estimativa local marcada com `~`.
- Tokens nunca vão para log, tela, área de transferência ou arquivo do app, e só são enviados ao domínio do próprio provedor.

## Consequências
- Às vezes o dado oficial fica indisponível até o usuário abrir a ferramenta, que renova o login sozinha.
- Nenhum risco de o monitor deslogar o usuário das IAs.
