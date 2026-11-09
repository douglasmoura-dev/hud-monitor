//! Every string the user reads on screen (HUD, mini bar, tray, scanner, tooltips).
//!
//! The interface speaks Portuguese; the rest of the code base speaks English. Keeping the visible
//! text in one module means a wording change touches one file and nothing else.

use crate::ai::AiError;
use crate::theme::ThemeId;

// ---------- waiting and shared reasons ----------

pub const WAITING_FIRST_READING: &str = "aguardando a primeira leitura";
pub const WAITING_FIRST_PING: &str = "aguardando a primeira medição";
pub const CLOCK_UNAVAILABLE: &str = "relógio indisponível";
pub const SYSTEM_CLOCK_UNAVAILABLE: &str = "relógio do sistema indisponível";
pub const HOME_OR_CLOCK_UNAVAILABLE: &str = "pasta do usuário ou relógio indisponível";
pub const NO_PROCESSES: &str = "processos indisponíveis";

pub fn collection_unavailable(cause: &str) -> String {
    format!("coleta indisponível: {cause}")
}

// ---------- metrics and sensors ----------

pub const DISK_IO_UNAVAILABLE: &str = "contador indisponível";
pub const CORES_UNAVAILABLE: &str = "contagem de núcleos indisponível";
pub const FREQ_UNAVAILABLE: &str = "frequência indisponível";
pub const SAVE_FAILED: &str = "não foi possível salvar o total do dia";
pub const PING_UNAVAILABLE: &str = "sem resposta";
pub const THERMAL_UNAVAILABLE: &str = "sensor indisponível";
pub const NO_NVIDIA_DRIVER: &str = "sem driver NVIDIA";
pub const GPU_IDLE: &str = "GPU em repouso";
pub const VRAM_UNAVAILABLE: &str = "VRAM indisponível neste driver";
pub const NVIDIA_FAILED: &str = "leitura NVIDIA falhou";
pub const INTEL_GPU_UNAVAILABLE: &str = "contador GPU Intel indisponível";
const SYSTEM_DISK_UNAVAILABLE: &str = "disco do sistema indisponível";

/// Reason shown when the system drive cannot be read; names the real drive when known.
pub fn disk_usage_unavailable(drive: Option<&str>) -> String {
    match drive {
        Some(d) => format!("disco {d} indisponível"),
        None => SYSTEM_DISK_UNAVAILABLE.to_owned(),
    }
}

// ---------- HUD rows ----------

pub fn cores_above_half(count: usize) -> String {
    match count {
        1 => "1 núcleo acima de 50%".to_owned(),
        n => format!("{n} núcleos acima de 50%"),
    }
}

pub fn net_today(size: &str) -> String {
    format!("hoje {size}")
}

pub const RESET_NOW: &str = "agora";
pub const DISK_CHIP_LABEL: &str = "DISCO";
pub const PRESENT: &str = "presente";
pub const RUNNING: &str = "ativo";
pub const NOT_RUNNING: &str = "inativo";

// ---------- AI providers ----------

pub const CLAUDE_NOT_FOUND: &str = "Claude Code não encontrado";
pub const CLAUDE_RENEW_LOGIN_TIP: &str = "abra o Claude Code para renovar o login";
pub const CODEX_NOT_FOUND: &str = "Codex não encontrado";
pub const CODEX_RENEW_LOGIN_TIP: &str = "abra o Codex para renovar o login";
pub const GEMINI_NOT_FOUND: &str = "Gemini CLI não encontrado";
pub const GEMINI_ESTIMATE_TIP: &str = "estimado pelos logs locais do Gemini CLI";
pub const CURSOR_NOT_FOUND: &str = "Cursor não encontrado";
pub const CURSOR_UNAVAILABLE: &str = "uso do Cursor indisponível";
pub const CURSOR_RENEW_LOGIN_TIP: &str = "abra o Cursor para renovar o login";
pub const UNREADABLE_RESPONSE: &str = "resposta ilegível";
pub const NO_LIMIT_WINDOWS: &str = "sem janelas de limite";
pub const PRIMARY_WINDOW: &str = "primária";
pub const SECONDARY_WINDOW: &str = "secundária";
/// Provider names with their Portuguese article, as used after "com".
pub const ANTHROPIC_WITH_ARTICLE: &str = "a Anthropic";
pub const CHATGPT_WITH_ARTICLE: &str = "o ChatGPT";

pub fn gemini_requests(used: u64, limit: u64, estimated: bool) -> String {
    let approx = if estimated { "~" } else { "" };
    format!("{approx}dia {used}/{limit}")
}

pub fn cursor_month_percent(percent: f32) -> String {
    format!("mês {percent:.0}%")
}

pub fn invalid_date(raw: &str) -> String {
    format!("data inválida `{raw}`")
}

pub fn missing_window(name: &str) -> String {
    format!("janela {name} ausente")
}

pub fn no_connection(provider: &str) -> String {
    format!("sem conexão com {provider}")
}

/// The reason shown in place of a value when a provider poll fails.
pub fn ai_error_reason(error: &AiError) -> String {
    match error {
        AiError::Credentials(detail) => format!("credenciais ilegíveis: {detail}"),
        AiError::Response(detail) => format!("resposta de limites inválida: {detail}"),
        AiError::Logs(detail) => format!("logs locais ilegíveis: {detail}"),
        AiError::Api(reason) => reason.clone(),
    }
}

// ---------- shell integration ----------

pub fn tray_unavailable(cause: &str) -> String {
    format!("bandeja indisponível: {cause}")
}

pub fn hotkeys_unavailable(cause: &str) -> String {
    format!("atalhos indisponíveis: {cause}")
}

pub const TRAY_TOOLTIP: &str = "HUD Monitor";
pub const TRAY_SHOW_HUD: &str = "Mostrar HUD";
pub const TRAY_SHOW_MINIBAR: &str = "Mostrar barrinha";
pub const TRAY_SCANNER: &str = "Limpeza de disco...";
pub const TRAY_PROFILES: &str = "Perfis";
pub const TRAY_THEME: &str = "Tema";
pub const TRAY_QUIT: &str = "Sair";
pub const TRAY_PIN: &str = "Fixar HUD (ignorar mouse)";
pub const PIN_TIP: &str =
    "Fixar: o HUD ignora o mouse. Para soltar, segure Ctrl e clique aqui, ou use a bandeja.";

/// Tray menu label of a theme.
pub fn theme_name(theme: ThemeId) -> &'static str {
    match theme {
        ThemeId::GamerHud => "Gamer HUD",
        ThemeId::Cyberpunk => "Cyberpunk",
        ThemeId::Minimal => "Minimalista",
        ThemeId::Terminal => "Terminal retrô",
        ThemeId::Light => "Claro",
    }
}

// ---------- theme identity ----------

pub const TITLE_GAMER: &str = "SYS // MONITOR";
pub const TITLE_CYBERPUNK: &str = "SYS_MONITOR";
pub const TITLE_MINIMAL: &str = "monitor";
pub const TITLE_TERMINAL: &str = "user@hud:~$";
pub const TITLE_LIGHT: &str = "Monitor";

/// Cyberpunk names of the section labels; labels not listed here are just joined with underscores.
const SNAKE_LABELS: [(&str, &str); 8] = [
    ("CPU", "CPU_LOAD"),
    ("RAM", "RAM_USED"),
    ("GPU", "GPU_CORE"),
    ("DISCO", "DISCO_USO"),
    ("REDE", "REDE_TRAFEGO"),
    ("INTEL", "GPU_INTEL"),
    ("NVIDIA", "GPU_NVIDIA"),
    ("VRAM", "VRAM_USED"),
];

/// `CPU_LOAD`-style form of a section label.
pub fn snake_label(key: &str) -> String {
    SNAKE_LABELS.iter().find(|(k, _)| *k == key).map_or_else(
        || key.to_uppercase().replace(' ', "_"),
        |(_, v)| (*v).to_string(),
    )
}

// ---------- disk cleanup scanner ----------

pub const NEEDS_ADMIN: &str = "precisa de admin";
pub const DOCKER_HINT: &str = "rode `docker system df` no terminal; o app não apaga nada do Docker";
pub const PARTIAL_READ_TIP: &str = "algumas pastas não puderam ser lidas; o total é no mínimo este";
pub const RECYCLE_BIN_NAME: &str = "Lixeira";
pub const CLEANUP_WORKER_LOST: &str = "a limpeza parou de forma inesperada; confira a Lixeira";
pub const NO_SYSTEM_ANSWER: &str = "sem resposta do sistema";
pub const SCANNING: &str = "Escaneando...";
pub const LINK_NOT_FOLLOWED: &str = "é um link ou junção; não é seguido";
pub const ALREADY_GONE: &str = "já não existe";
pub const IN_USE_OR_DENIED: &str = "em uso por outro programa ou sem permissão";
pub const BIN_EMPTIED: &str = "Lixeira esvaziada";
pub const INTERRUPTED: &str = "interrompido";

pub fn item_count(count: u64) -> String {
    format!("{count} itens")
}

pub fn missing_env_var(name: &str) -> String {
    format!("variável {name} ausente")
}

pub fn categories_measured(count: usize) -> String {
    format!("{count} categorias medidas")
}

pub fn confirm_move_to_bin(total: &str) -> String {
    format!("Mover para a Lixeira ({total})? Dá para restaurar pela Lixeira.")
}

pub fn moving_to_bin(done: usize, total: usize) -> String {
    format!("Movendo para a Lixeira... {done} de {total}")
}

pub fn freed(size: &str) -> String {
    format!("Liberados {size}")
}

pub fn items_moved_to_bin(count: u64) -> String {
    format!("{count} itens movidos para a Lixeira")
}

pub fn skipped_count(count: usize) -> String {
    format!("{count} não foi possível mover")
}

pub fn more_skipped(count: usize) -> String {
    format!("… e mais {count}")
}

pub mod category {
    pub const TEMP: &str = "Temporários";
    pub const RECYCLE_BIN: &str = "Lixeira";
    pub const BROWSER_CACHE: &str = "Cache de navegadores";
    pub const NODE_MODULES: &str = "node_modules antigos";
    pub const PACKAGE_CACHES: &str = "Caches de pacotes";
    pub const WINDOWS_UPDATE: &str = "Windows Update";
    pub const DOCKER: &str = "Docker";
    pub const OLD_DOWNLOADS: &str = "Downloads antigos";
}

// ---------- recycle bin failures (Windows adapter) ----------

pub fn recycle_bin_unavailable(cause: &str) -> String {
    format!("Lixeira indisponível: {cause}")
}

pub fn com_unavailable(cause: &str) -> String {
    format!("COM indisponível: {cause}")
}

pub fn file_operation_unavailable(cause: &str) -> String {
    format!("IFileOperation indisponível: {cause}")
}

pub fn operation_flags_failed(cause: &str) -> String {
    format!("flags da operação: {cause}")
}

pub fn recycle_bin_refused(cause: &str) -> String {
    format!("Lixeira recusou a operação: {cause}")
}

pub fn recycle_bin_not_emptied(cause: &str) -> String {
    format!("Lixeira não pôde ser esvaziada: {cause}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_reasons_are_portuguese() {
        assert_eq!(THERMAL_UNAVAILABLE, "sensor indisponível");
        assert_eq!(NO_NVIDIA_DRIVER, "sem driver NVIDIA");
        assert_eq!(CLAUDE_NOT_FOUND, "Claude Code não encontrado");
        assert_eq!(CODEX_RENEW_LOGIN_TIP, "abra o Codex para renovar o login");
        assert_eq!(WAITING_FIRST_READING, "aguardando a primeira leitura");
        assert_eq!(NEEDS_ADMIN, "precisa de admin");
    }

    #[test]
    fn disk_reason_names_the_drive_when_known() {
        assert_eq!(disk_usage_unavailable(Some("D:")), "disco D: indisponível");
        assert_eq!(
            disk_usage_unavailable(None),
            "disco do sistema indisponível"
        );
    }

    #[test]
    fn counts_use_the_right_plural() {
        assert_eq!(cores_above_half(1), "1 núcleo acima de 50%");
        assert_eq!(cores_above_half(3), "3 núcleos acima de 50%");
    }

    #[test]
    fn ai_errors_keep_their_portuguese_prefix_on_screen() {
        let credentials = AiError::Credentials("x".into());
        let response = AiError::Response("y".into());
        let logs = AiError::Logs("z".into());
        let api = AiError::Api("sem conexão".into());
        assert_eq!(ai_error_reason(&credentials), "credenciais ilegíveis: x");
        assert_eq!(
            ai_error_reason(&response),
            "resposta de limites inválida: y"
        );
        assert_eq!(ai_error_reason(&logs), "logs locais ilegíveis: z");
        assert_eq!(ai_error_reason(&api), "sem conexão");
    }

    #[test]
    fn provider_row_texts_match_the_hud() {
        assert_eq!(gemini_requests(3, 10, true), "~dia 3/10");
        assert_eq!(gemini_requests(3, 10, false), "dia 3/10");
        assert_eq!(cursor_month_percent(64.2), "mês 64%");
    }

    #[test]
    fn scanner_texts_match_the_confirmation_flow() {
        assert_eq!(
            confirm_move_to_bin("1.0 GB"),
            "Mover para a Lixeira (1.0 GB)? Dá para restaurar pela Lixeira."
        );
        assert_eq!(moving_to_bin(2, 5), "Movendo para a Lixeira... 2 de 5");
        assert_eq!(categories_measured(4), "4 categorias medidas");
    }

    #[test]
    fn shell_notices_are_portuguese() {
        assert_eq!(tray_unavailable("x"), "bandeja indisponível: x");
        assert_eq!(hotkeys_unavailable("y"), "atalhos indisponíveis: y");
    }
}
