# hud-monitor

Monitor de sistema para Windows em estilo gamer HUD: transparente, sempre por cima, arrastável e leve (meta ≤ 30 MB de RAM).

- CPU por núcleo, temperatura, RAM, GPU, disco, rede e ping
- Limites das IAs instaladas: Claude Pro/Max (5 h e semanal), Codex, Gemini, Cursor
- HUD completo + barrinha configurável, perfis, bandeja e atalhos
- Scanner de limpeza que só move para a Lixeira

Feito em Rust + Slint. Especificação em [specs/001-hud-monitor](specs/001-hud-monitor/spec.md).

## Licença

Código sob a [licença MIT](LICENSE): livre para usar, copiar, modificar e distribuir, inclusive comercialmente, mantendo o aviso de copyright.

A interface usa o [Slint](https://slint.dev) sob a licença Royalty-free do Slint.

As fontes Rajdhani e Share Tech Mono embutidas seguem a [SIL Open Font License 1.1](crates/hud-app/ui/fonts/) de cada uma.
