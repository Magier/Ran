mod builtin;
mod executor;
mod output;
mod shell_session;
mod types;

pub use executor::{
    C2Backend, C2EventBus, C2EventReceiver, C2EventRecvError, C2Handle, C2Manager, C2RuntimeLimits,
    DEFAULT_MAX_CONCURRENT_EXECUTIONS,
};
pub use output::{OutputSink, OutputStream};
pub use shell_session::ShellSession;
pub use types::{
    C2Event, ExecTtp, ExecutionOperation, OutputTransform, SessionConnectedData, TtpExecuted,
    BUILTIN_C2_ID, DEFAULT_EXECUTION_TIMEOUT_SECONDS,
};
