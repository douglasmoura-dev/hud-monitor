# ADR 0001 — Rust + Slint (renderizador de software)

**Status:** aceita · 2026-11-07

## Contexto
O app fica aberto o dia todo num notebook com 2 núcleos e pouca folga. O usuário exigiu que ele seja leve em RAM. Também precisa de janela sem borda, transparente, sempre por cima e arrastável.

## Opções
| Opção | RAM estimada | Contra |
|---|---|---|
| C# + WPF | 50–90 MB | Runtime .NET e WPF residentes |
| Rust + Tauri | 80–150 MB | Processos do WebView2 |
| **Rust + Slint (software)** | **10–25 MB** | Temperatura da CPU sem biblioteca pronta |

## Decisão
Rust estável com Slint usando `renderer-software` e `backend-winit`.

## Consequências
- Meta de memória privada ≤ 30 MB vira requisito testável.
- Temperatura vem do contador PDH de zona térmica (sem admin). Leitura exata por núcleo exige driver e fica fora da v1.
- Precisamos escrever à mão a integração com DWM (acrílico) e com a Shell (Lixeira) via crate `windows`.
- Trocar de stack depois significa reescrever a UI inteira: decisão cara de reverter.
