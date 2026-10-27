use super::{
    allocator::BudgetAllocator,
    host_api::{self, Host, HostSetup},
    stopped_at_limit, Call, Shared,
};
use crate::source_host::{runtime::source_error, types::*};
use rquickjs::{
    context::intrinsic, function::This, promise::MaybePromise, AsyncContext, AsyncRuntime,
    CatchResultExt, CaughtError, Ctx, Module, Object, Persistent, Value, WriteOptions,
};
use std::{rc::Rc, sync::Arc};
use tokio::sync::{mpsc, oneshot};

pub(super) const THREAD_STACK: usize = 8 * 1024 * 1024;
const SCRIPT_STACK: usize = 1024 * 1024;
const MAX_RESULT: usize = 1024 * 1024;

/// The language built-ins a source context gets: no `eval` and no `Function`
/// constructors. Without the `Eval` intrinsic QuickJS has no compiler in this
/// context, so `eval` and every function constructor throw. The host API is
/// added on top by `host_api::install`.
type Sandbox = (
    intrinsic::Date,
    intrinsic::RegExpCompiler,
    intrinsic::RegExp,
    intrinsic::Json,
    intrinsic::Proxy,
    intrinsic::MapSet,
    intrinsic::TypedArrays,
    intrinsic::Promise,
    intrinsic::WeakRef,
);
type Compiler = (intrinsic::Eval, intrinsic::RegExpCompiler);

pub(super) struct Launch {
    pub(super) shared: Arc<Shared>,
    pub(super) source: Vec<u8>,
    pub(super) name: String,
    pub(super) memory_limit: usize,
    pub(super) host: HostSetup,
    pub(super) calls: mpsc::UnboundedReceiver<Call>,
    pub(super) ready: oneshot::Sender<HostResult<()>>,
}

pub(super) fn run(launch: Launch) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            launch.shared.terminate();
            return;
        }
    };
    tokio::task::LocalSet::new().block_on(&runtime, serve(launch));
}

async fn serve(launch: Launch) {
    let Launch {
        shared,
        source,
        name,
        memory_limit,
        host,
        mut calls,
        ready,
    } = launch;
    let host = Host::new(host, shared.shutdown.clone());
    let engine = match Engine::load(&shared, &name, source, memory_limit, host).await {
        Ok(engine) => Rc::new(engine),
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    if ready.send(Ok(())).is_err() {
        return;
    }
    loop {
        tokio::select! {
            _ = shared.shutdown.cancelled() => return,
            call = calls.recv() => match call {
                Some(call) => {
                    tokio::task::spawn_local(engine.clone().dispatch(shared.clone(), call));
                }
                None => return,
            },
        }
    }
}

enum Outcome {
    Settled(HostResult<serde_json::Value>),
    Limit,
}

/// Field order matters: the exports must drop before the context that owns them.
struct Engine {
    exports: Persistent<Object<'static>>,
    context: AsyncContext,
    host: Rc<Host>,
}
impl Engine {
    async fn load(
        shared: &Arc<Shared>,
        name: &str,
        source: Vec<u8>,
        memory_limit: usize,
        host: Rc<Host>,
    ) -> HostResult<Self> {
        let failed = || {
            if shared.limit_reached() {
                stopped_at_limit()
            } else {
                HostError::new(
                    "source_unavailable",
                    "Source code cannot load. Install a source update.",
                )
            }
        };
        let runtime =
            AsyncRuntime::new_with_alloc(BudgetAllocator::new(memory_limit, shared.clone()))
                .map_err(|_| failed())?;
        runtime.set_max_stack_size(SCRIPT_STACK).await;
        let watched = shared.clone();
        runtime
            .set_interrupt_handler(Some(Box::new(move || watched.limit_reached())))
            .await;
        let compiler = AsyncContext::custom::<Compiler>(&runtime)
            .await
            .map_err(|_| failed())?;
        let bytecode = compiler
            .with(|ctx| Module::declare(ctx, name, source)?.write(WriteOptions::default()))
            .await
            .map_err(|_| failed())?;
        drop(compiler);
        let context = AsyncContext::custom::<Sandbox>(&runtime)
            .await
            .map_err(|_| failed())?;
        let installed = host.clone();
        context
            .with(|ctx| host_api::install(&ctx, &installed))
            .await
            .map_err(|_| failed())?;
        let exports = context
            .async_with(
                async |ctx| -> rquickjs::Result<Persistent<Object<'static>>> {
                    // SAFETY: the bytes are module bytecode that `Module::write` produced
                    // above in this runtime from the verified entry file.
                    let module = unsafe { Module::load(ctx.clone(), &bytecode) }?;
                    let (module, evaluated) = module.eval()?;
                    evaluated.into_future::<()>().await?;
                    let exports: Object = module.get("default")?;
                    Ok(Persistent::save(&ctx, exports))
                },
            )
            .await
            .map_err(|_| failed())?;
        Ok(Self {
            exports,
            context,
            host,
        })
    }
    async fn dispatch(self: Rc<Self>, shared: Arc<Shared>, call: Call) {
        let exports = self.exports.clone();
        let Call { id, method, params } = call;
        self.host.observe_credentials(&params);
        if method == "auth.sign_out" {
            self.host.forget_cookies();
        }
        let outcome = self
            .context
            .async_with(async |ctx| invoke(&ctx, exports, &method, params).await)
            .await;
        if let Outcome::Settled(Ok(result)) = &outcome {
            self.host.observe_credentials(result);
        }
        match outcome {
            Outcome::Settled(result) if !shared.limit_reached() => shared.settle(id, result),
            _ => shared.terminate(),
        }
    }
}

async fn invoke<'js>(
    ctx: &Ctx<'js>,
    exports: Persistent<Object<'static>>,
    method: &str,
    params: serde_json::Value,
) -> Outcome {
    let called = async {
        let exports = exports.restore(ctx)?;
        let Some(function) = exports.get::<_, Value>(method)?.into_function() else {
            return Ok(None);
        };
        let params = ctx.json_parse(params.to_string())?;
        let returned: MaybePromise = function.call((This(exports), params))?;
        let value: Value = returned.into_future().await?;
        let json = ctx.json_stringify(value)?;
        json.map(|json| json.to_string()).transpose().map(Some)
    }
    .await
    .catch(ctx);
    match called {
        Ok(None) => Outcome::Settled(Err(HostError::new(
            "unsupported_operation",
            "This source operation is not supported.",
        ))),
        Ok(Some(json)) => Outcome::Settled(decode(json)),
        Err(CaughtError::Exception(exception)) => {
            let value = exception.clone().into_value();
            let message = exception.message();
            if value.is_uncatchable_error()
                || matches!(
                    message.as_deref(),
                    Some("out of memory" | "Maximum call stack size exceeded")
                )
            {
                Outcome::Limit
            } else {
                Outcome::Settled(Err(thrown(value)))
            }
        }
        Err(CaughtError::Value(value)) if value.is_uncatchable_error() => Outcome::Limit,
        Err(CaughtError::Value(value)) => Outcome::Settled(Err(thrown(value))),
        Err(CaughtError::Error(_)) => Outcome::Settled(Err(internal())),
    }
}

fn decode(json: Option<String>) -> HostResult<serde_json::Value> {
    let Some(json) = json else {
        return Ok(serde_json::Value::Null);
    };
    if json.len() > MAX_RESULT {
        return Err(HostError::new(
            "internal",
            "Source result exceeds the protocol size limit. Update the source.",
        ));
    }
    serde_json::from_str(&json).map_err(|_| internal())
}

fn thrown(value: Value<'_>) -> HostError {
    value
        .as_object()
        .and_then(|object| object.get::<_, String>("code").ok())
        .and_then(|code| source_error(&code))
        .unwrap_or_else(internal)
}

fn internal() -> HostError {
    HostError::new(
        "internal",
        "Source request failed (internal). Retry, sign in, or update the source as appropriate.",
    )
}
