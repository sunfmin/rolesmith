//! The one place commands are registered (SPEC.md §8).
//!
//! A command is a module here with a `pub struct Args` that derives `clap::Args` and implements
//! [`Command`]. Adding one is a `pub mod` line and a line in `registry!` below; nothing else in
//! the frame changes. The parser, the dispatch and `agent-context` are all generated from that
//! list, so they cannot disagree.

use serde_json::Value;

pub mod agent_context;
pub mod check;
pub mod doctor;
pub mod list;
pub mod prompt;
pub mod show;
pub mod source;
pub mod tool;

/// What a successful command produced, in both of its forms.
pub struct Output {
    /// The one JSON document `--json` prints. Its shape is [`Command::output_shape`].
    pub json: Value,
    /// The human text printed otherwise.
    pub human: String,
}

/// Why a command refused or failed: printed on stderr, exit 1.
pub struct Failure {
    pub message: String,
    /// When the refusal is a list (`check`'s problems, `doctor`'s missing things), its key under
    /// `error` and the items; with `--json` they are `error.<key>`.
    pub list: Option<(&'static str, Value)>,
}

impl Failure {
    pub fn new(message: impl Into<String>) -> Self {
        Failure {
            message: message.into(),
            list: None,
        }
    }

    /// A refusal that is a list: `header`, then each item on its own indented line, and the
    /// items as JSON under `error.<key>`.
    pub fn listing<T: std::fmt::Display>(
        header: String,
        key: &'static str,
        items: &[T],
        to_json: fn(&T) -> Value,
    ) -> Self {
        let mut message = header;
        for item in items {
            message.push_str(&format!("\n  {item}"));
        }
        Failure {
            message,
            list: Some((key, items.iter().map(to_json).collect())),
        }
    }
}

/// A registered command.
pub trait Command: clap::Args {
    /// The name the command is typed as.
    const NAME: &'static str;

    /// The JSON document `--json` prints on success, as a JSON Schema.
    fn output_shape() -> Value;

    fn run(self) -> Result<Output, Failure>;
}

macro_rules! registry {
    ($($variant:ident => $module:ident),+ $(,)?) => {
        /// Every registered command, as the parser knows them.
        #[derive(clap::Subcommand)]
        pub enum Registered {
            $(
                #[command(name = <$module::Args as Command>::NAME)]
                $variant($module::Args),
            )+
        }

        impl Registered {
            pub fn run(self) -> Result<Output, Failure> {
                match self {
                    $(Self::$variant(args) => Command::run(args),)+
                }
            }
        }

        /// Each registered command's name with its output shape.
        pub fn output_shapes() -> Vec<(&'static str, Value)> {
            vec![$((
                <$module::Args as Command>::NAME,
                <$module::Args as Command>::output_shape(),
            ),)+]
        }
    };
}

registry! {
    AgentContext => agent_context,
    Check => check,
    Doctor => doctor,
    List => list,
    Prompt => prompt,
    Show => show,
    Source => source,
    Tool => tool,
}
