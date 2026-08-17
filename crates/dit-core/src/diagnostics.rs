//! Health checks — what `dit doctor` prints and the server reports.

/// How serious a finding is. `Error` means the next write will fail or data
/// is at risk; `Warn` means degraded but working; `Ok` is a check that
/// passed and is worth saying so about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticLevel {
    Ok,
    Warn,
    Error,
}

/// One finding. `code` is a stable identifier so output can be grepped and
/// tests can assert on a check without matching prose.
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub code: &'static str,
    pub message: String,
}

impl Diagnostic {
    pub fn ok(code: &'static str, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            level: DiagnosticLevel::Ok,
            code,
            message: message.into(),
        }
    }

    pub fn warn(code: &'static str, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            level: DiagnosticLevel::Warn,
            code,
            message: message.into(),
        }
    }

    pub fn error(code: &'static str, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            level: DiagnosticLevel::Error,
            code,
            message: message.into(),
        }
    }
}
