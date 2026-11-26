//! The one place commands are registered.
//!
//! A command is a module here with a `pub struct Args` that derives `clap::Args` and implements
//! [`Command`]; its help is the Markdown doc comment on that struct (`verbatim_doc_comment`), and
//! its group, who runs it, is one of [`groups`], beside it in `registry!`. Adding one is a `pub mod` line and a line in
//! `registry!` below; nothing else in the frame changes. The parser, the dispatch, the manual and
//! `agent-context` are all generated from that list, in its order, so they cannot disagree.

use serde_json::Value;

pub mod agent_context;
pub mod check;
pub mod distill;
pub mod doctor;
pub mod env;
pub mod lesson;
pub mod list;
pub mod new;
pub mod prompt;
pub mod propose;
pub mod remove;
pub mod show;
pub mod source;
pub mod status;
pub mod suggest;
pub mod sync;
pub mod tool;
pub mod update;
pub mod use_;

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

/// Who runs a command: the groups the manual lists commands under, in the order it first meets
/// them.
pub mod groups {
    /// Commands that describe rolesmith itself; `help` is one, so this group is first.
    pub const LEARNING: &str = "Run by an agent learning rolesmith";
    /// Commands that keep the libraries this machine knows.
    pub const MACHINE: &str = "Run on a machine, to know its libraries";
    /// Commands that choose and install a project's roles.
    pub const PROJECT: &str = "Run by whoever sets up a project's agents";
    /// Commands an agent runs while it works under a role.
    pub const ROLE: &str = "Run by an agent working under a role";
    /// Commands that change a library's roles.
    pub const LIBRARY: &str = "Run by an agent writing a library's roles";
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
    ($($variant:ident => $module:ident in $group:ident),+ $(,)?) => {
        /// Every registered command, as the parser knows them.
        #[derive(clap::Subcommand)]
        pub enum Registered {
            $(
                #[command(
                    name = <$module::Args as Command>::NAME,
                    next_help_heading = groups::$group
                )]
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
    AgentContext => agent_context in LEARNING,
    Source => source in MACHINE,
    List => list in PROJECT,
    Show => show in PROJECT,
    Suggest => suggest in PROJECT,
    Use => use_ in PROJECT,
    Sync => sync in PROJECT,
    Status => status in PROJECT,
    Update => update in PROJECT,
    Remove => remove in PROJECT,
    Prompt => prompt in ROLE,
    Doctor => doctor in ROLE,
    Tool => tool in ROLE,
    Env => env in ROLE,
    Lesson => lesson in ROLE,
    New => new in LIBRARY,
    Check => check in LIBRARY,
    Distill => distill in LIBRARY,
    Propose => propose in LIBRARY,
}
