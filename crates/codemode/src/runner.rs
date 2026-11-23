// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Stateless pure-Rust `MontyRepl` execution driver with capability containment,
//! separate interpreter feed vs wall-clock deadlines, dual host/mutation call
//! budgets, and lossless output spilling.

use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use monty::{MontyRepl, ReplProgress, ReplStartError};
use monty_types::{
    CompileOptions, ExcType, ExtFunctionResult, MontyException, MontyObject, NameLookupResult,
    NamedValues, OsPolicy, PrintWriter, ResourceLimits, ResourceTracker, SleepMode,
};
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::value::RawValue;
use serde_json::{Value, json};

use super::backend::TcCodeBackend;
use super::catalog::{OpClass, TcCodeOp};
use super::value::{drop_json_iteratively, json_to_monty, kwargs_to_json_object, monty_to_json};
use super::{ExecuteErrorInfo, ExecuteOutcome, ExecuteRequest, ExecuteStatus, TcCodeLimits};

/// Maximum spill files retained per process before evicting the oldest.
const MAX_RETAINED_SPILL_FILES: usize = 32;

static SPILL_RING: LazyLock<Mutex<VecDeque<PathBuf>>> =
    LazyLock::new(|| Mutex::new(VecDeque::with_capacity(MAX_RETAINED_SPILL_FILES)));

/// Stateless Code Mode runner holding default limits.
#[derive(Debug, Clone, Default)]
pub struct TcCodeRunner {
    limits: TcCodeLimits,
}

impl TcCodeRunner {
    #[must_use]
    pub fn new(limits: TcCodeLimits) -> Self {
        Self {
            limits: limits.clamped(),
        }
    }

    #[must_use]
    pub const fn limits(&self) -> TcCodeLimits {
        self.limits
    }

    pub fn execute(&self, code: &str, backend: &dyn TcCodeBackend) -> ExecuteOutcome {
        execute_script(
            &ExecuteRequest {
                code: code.to_owned(),
                limits: self.limits,
                spill_dir: None,
            },
            backend,
        )
    }
}

/// Runs [`execute_script`] on a dedicated Tokio blocking worker thread.
///
/// Keeps synchronous `MontyRepl` execution and `Handle::block_on` IPC calls off
/// Tokio async worker threads and guarantees a full OS thread stack.
pub async fn execute_script_async<B>(
    request: ExecuteRequest,
    backend: B,
) -> Result<ExecuteOutcome, String>
where
    B: TcCodeBackend + 'static,
{
    tokio::task::spawn_blocking(move || execute_script(&request, &backend))
        .await
        .map_err(|err| format!("codemode worker task panicked or was cancelled: {err}"))
}

fn elapsed_ms_u64(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Executes one Python snippet inside a fresh, zero-egress `MontyRepl`.
#[allow(clippy::too_many_lines)]
pub fn execute_script(request: &ExecuteRequest, backend: &dyn TcCodeBackend) -> ExecuteOutcome {
    let started = Instant::now();
    let limits = request.limits.clamped();
    let max_wall_duration = limits.max_wall_duration;

    let resource_limits = ResourceLimits::default()
        .max_feed_duration(limits.max_feed_duration)
        .max_turn_duration(limits.max_feed_duration)
        .max_memory(limits.max_memory_bytes)
        .max_recursion_depth(limits.max_recursion_depth)
        .max_suspensions(limits.max_host_calls.saturating_mul(4).max(64));

    let tracker = ResourceTracker::new(resource_limits);
    let os_policy = OsPolicy {
        sleep: SleepMode::Zero,
        ..OsPolicy::default()
    };

    let repl =
        MontyRepl::new("<tc-code>", tracker, CompileOptions::default()).with_os_policy(os_policy);

    let mut stdout_buf = String::new();
    // Cap collected stdout at 4 MiB in-memory before spill/truncation handling.
    let max_print_bytes = limits.max_memory_bytes.min(4 * 1024 * 1024);
    let mut host_calls = 0usize;
    let mut mutation_calls = 0usize;
    let mut suspension_steps = 0usize;
    let max_suspension_steps = limits.max_host_calls.saturating_mul(8).max(128);

    let mut progress = match repl.feed_start(
        &request.code,
        NamedValues::new(),
        PrintWriter::CollectString(&mut stdout_buf, Some(max_print_bytes)),
    ) {
        Ok(p) => p,
        Err(err) => {
            return build_error_outcome(
                err,
                stdout_buf,
                host_calls,
                mutation_calls,
                elapsed_ms_u64(started),
                &limits,
                request.spill_dir.as_deref(),
            );
        }
    };

    loop {
        match progress {
            ReplProgress::Complete { repl: _repl, value } => {
                let duration_ms = elapsed_ms_u64(started);
                return match monty_to_json(&value) {
                    Ok(json_val) => finalize_ok_outcome(
                        json_val,
                        stdout_buf,
                        host_calls,
                        mutation_calls,
                        duration_ms,
                        &limits,
                        request.spill_dir.as_deref(),
                    ),
                    Err(conv_err) => build_conversion_error_outcome(
                        conv_err.to_string(),
                        stdout_buf,
                        host_calls,
                        mutation_calls,
                        duration_ms,
                        &limits,
                        request.spill_dir.as_deref(),
                    ),
                };
            }
            ReplProgress::NameLookup(lookup) => {
                suspension_steps += 1;
                if started.elapsed() > max_wall_duration {
                    let exc = wall_timeout_exception(max_wall_duration);
                    match lookup.abort(
                        exc,
                        PrintWriter::CollectString(&mut stdout_buf, Some(max_print_bytes)),
                    ) {
                        Ok(next) => progress = next,
                        Err(err) => {
                            return build_error_outcome(
                                err,
                                stdout_buf,
                                host_calls,
                                mutation_calls,
                                elapsed_ms_u64(started),
                                &limits,
                                request.spill_dir.as_deref(),
                            );
                        }
                    }
                    continue;
                }
                if suspension_steps > max_suspension_steps {
                    let exc = MontyException::new(
                        ExcType::RuntimeError,
                        Some(format!(
                            "sandbox suspension limit exceeded ({max_suspension_steps})"
                        )),
                    );
                    match lookup.abort(
                        exc,
                        PrintWriter::CollectString(&mut stdout_buf, Some(max_print_bytes)),
                    ) {
                        Ok(next) => progress = next,
                        Err(err) => {
                            return build_error_outcome(
                                err,
                                stdout_buf,
                                host_calls,
                                mutation_calls,
                                elapsed_ms_u64(started),
                                &limits,
                                request.spill_dir.as_deref(),
                            );
                        }
                    }
                    continue;
                }

                let answer = if lookup.object_id().is_none() {
                    TcCodeOp::from_name(&lookup.name).map_or(NameLookupResult::Undefined, |op| {
                        let doc =
                            format!("{}\n{}", op.signature_one_liner(), op.descriptor().summary);
                        NameLookupResult::Value(MontyObject::function(op.as_str(), Some(doc)))
                    })
                } else {
                    NameLookupResult::Undefined
                };

                progress = match lookup.resume(
                    answer,
                    PrintWriter::CollectString(&mut stdout_buf, Some(max_print_bytes)),
                ) {
                    Ok(next) => next,
                    Err(err) => {
                        return build_error_outcome(
                            err,
                            stdout_buf,
                            host_calls,
                            mutation_calls,
                            elapsed_ms_u64(started),
                            &limits,
                            request.spill_dir.as_deref(),
                        );
                    }
                };
            }
            ReplProgress::FunctionCall(call) => {
                suspension_steps += 1;
                if started.elapsed() > max_wall_duration {
                    let exc = wall_timeout_exception(max_wall_duration);
                    match call.abort(
                        exc,
                        PrintWriter::CollectString(&mut stdout_buf, Some(max_print_bytes)),
                    ) {
                        Ok(next) => progress = next,
                        Err(err) => {
                            return build_error_outcome(
                                err,
                                stdout_buf,
                                host_calls,
                                mutation_calls,
                                elapsed_ms_u64(started),
                                &limits,
                                request.spill_dir.as_deref(),
                            );
                        }
                    }
                    continue;
                }
                if host_calls >= limits.max_host_calls {
                    let exc = MontyException::new(
                        ExcType::RuntimeError,
                        Some(format!(
                            "host call limit exceeded: max_host_calls={} reached before calling '{}'",
                            limits.max_host_calls, call.function_name
                        )),
                    );
                    match call.abort(
                        exc,
                        PrintWriter::CollectString(&mut stdout_buf, Some(max_print_bytes)),
                    ) {
                        Ok(next) => progress = next,
                        Err(err) => {
                            return build_error_outcome(
                                err,
                                stdout_buf,
                                host_calls,
                                mutation_calls,
                                elapsed_ms_u64(started),
                                &limits,
                                request.spill_dir.as_deref(),
                            );
                        }
                    }
                    continue;
                }

                let Some(op) = TcCodeOp::from_name(&call.function_name) else {
                    let result = ExtFunctionResult::NotFound(call.function_name.clone());
                    progress = match call.resume(
                        result,
                        PrintWriter::CollectString(&mut stdout_buf, Some(max_print_bytes)),
                    ) {
                        Ok(next) => next,
                        Err(err) => {
                            return build_error_outcome(
                                err,
                                stdout_buf,
                                host_calls,
                                mutation_calls,
                                elapsed_ms_u64(started),
                                &limits,
                                request.spill_dir.as_deref(),
                            );
                        }
                    };
                    continue;
                };

                let is_mutate = op.op_class() == OpClass::Mutate;
                if is_mutate && mutation_calls >= limits.max_mutation_calls {
                    let exc = MontyException::new(
                        ExcType::RuntimeError,
                        Some(format!(
                            "mutation call limit exceeded ({}): split batch mutations or pass explicit max_mutations",
                            limits.max_mutation_calls
                        )),
                    );
                    match call.abort(
                        exc,
                        PrintWriter::CollectString(&mut stdout_buf, Some(max_print_bytes)),
                    ) {
                        Ok(next) => progress = next,
                        Err(err) => {
                            return build_error_outcome(
                                err,
                                stdout_buf,
                                host_calls,
                                mutation_calls,
                                elapsed_ms_u64(started),
                                &limits,
                                request.spill_dir.as_deref(),
                            );
                        }
                    }
                    continue;
                }

                let sig_hint = op.signature_one_liner();
                let ext_result = match kwargs_to_json_object(op.as_str(), &call.args) {
                    Err(arg_err) => {
                        ExtFunctionResult::Error(arg_err.into_monty_exception(Some(&sig_hint)))
                    }
                    Ok(kwargs) => {
                        host_calls += 1;
                        if is_mutate {
                            mutation_calls += 1;
                        }
                        let remaining_wall = max_wall_duration.saturating_sub(started.elapsed());
                        match backend.execute_op(op, kwargs, remaining_wall) {
                            Ok(val) => {
                                let converted = json_to_monty(&val);
                                drop_json_iteratively(val);
                                match converted {
                                    Ok(obj) => ExtFunctionResult::Return(obj),
                                    Err(val_err) => {
                                        ExtFunctionResult::Error(val_err.into_monty_exception(None))
                                    }
                                }
                            }
                            Err(backend_err) => {
                                let exc = if backend_err.kind == "type_error" {
                                    MontyException::new(
                                        ExcType::TypeError,
                                        Some(backend_err.message),
                                    )
                                } else {
                                    MontyException::new(
                                        ExcType::RuntimeError,
                                        Some(backend_err.to_string()),
                                    )
                                };
                                ExtFunctionResult::Error(exc)
                            }
                        }
                    }
                };

                if started.elapsed() > max_wall_duration {
                    let exc = wall_timeout_exception(max_wall_duration);
                    progress = match call.abort(
                        exc,
                        PrintWriter::CollectString(&mut stdout_buf, Some(max_print_bytes)),
                    ) {
                        Ok(next) => next,
                        Err(err) => {
                            return build_error_outcome(
                                err,
                                stdout_buf,
                                host_calls,
                                mutation_calls,
                                elapsed_ms_u64(started),
                                &limits,
                                request.spill_dir.as_deref(),
                            );
                        }
                    };
                } else {
                    progress = match call.resume(
                        ext_result,
                        PrintWriter::CollectString(&mut stdout_buf, Some(max_print_bytes)),
                    ) {
                        Ok(next) => next,
                        Err(err) => {
                            return build_error_outcome(
                                err,
                                stdout_buf,
                                host_calls,
                                mutation_calls,
                                elapsed_ms_u64(started),
                                &limits,
                                request.spill_dir.as_deref(),
                            );
                        }
                    };
                }
            }
            ReplProgress::OsCall(os_call) => {
                suspension_steps += 1;
                if started.elapsed() > max_wall_duration {
                    let exc = wall_timeout_exception(max_wall_duration);
                    match os_call.abort(
                        exc,
                        PrintWriter::CollectString(&mut stdout_buf, Some(max_print_bytes)),
                    ) {
                        Ok(next) => progress = next,
                        Err(err) => {
                            return build_error_outcome(
                                err,
                                stdout_buf,
                                host_calls,
                                mutation_calls,
                                elapsed_ms_u64(started),
                                &limits,
                                request.spill_dir.as_deref(),
                            );
                        }
                    }
                    continue;
                }
                if suspension_steps > max_suspension_steps {
                    let exc = MontyException::new(
                        ExcType::RuntimeError,
                        Some(format!(
                            "sandbox suspension limit exceeded ({max_suspension_steps})"
                        )),
                    );
                    match os_call.abort(
                        exc,
                        PrintWriter::CollectString(&mut stdout_buf, Some(max_print_bytes)),
                    ) {
                        Ok(next) => progress = next,
                        Err(err) => {
                            return build_error_outcome(
                                err,
                                stdout_buf,
                                host_calls,
                                mutation_calls,
                                elapsed_ms_u64(started),
                                &limits,
                                request.spill_dir.as_deref(),
                            );
                        }
                    }
                    continue;
                }

                // Zero-egress capability containment: all OS / FS / env / import calls
                // receive Monty's canonical `no_handler_result()` (`PermissionError` on FS,
                // `ModuleNotFoundError` on external imports, `RuntimeError` on unsupported OS).
                progress = match os_call.resume_with(
                    PrintWriter::CollectString(&mut stdout_buf, Some(max_print_bytes)),
                    |fc| fc.no_handler_result(),
                ) {
                    Ok(next) => next,
                    Err(err) => {
                        return build_error_outcome(
                            err,
                            stdout_buf,
                            host_calls,
                            mutation_calls,
                            elapsed_ms_u64(started),
                            &limits,
                            request.spill_dir.as_deref(),
                        );
                    }
                };
            }
            ReplProgress::ResolveFutures(futures) => {
                let exc = MontyException::new(
                    ExcType::RuntimeError,
                    Some(
                        "unresolved external futures are not supported in synchronous Code Mode"
                            .to_owned(),
                    ),
                );
                match futures.abort(
                    exc,
                    PrintWriter::CollectString(&mut stdout_buf, Some(max_print_bytes)),
                ) {
                    Ok(next) => progress = next,
                    Err(err) => {
                        return build_error_outcome(
                            err,
                            stdout_buf,
                            host_calls,
                            mutation_calls,
                            elapsed_ms_u64(started),
                            &limits,
                            request.spill_dir.as_deref(),
                        );
                    }
                }
            }
        }
    }
}

fn wall_timeout_exception(max_wall: Duration) -> MontyException {
    MontyException::new(
        ExcType::TimeoutError,
        Some(format!(
            "total execution wall-clock limit exceeded ({}ms)",
            max_wall.as_millis()
        )),
    )
}

fn classify_exception_kind(exc: &MontyException) -> &'static str {
    let ty = exc.exc_type().to_string();
    let msg = exc.message().unwrap_or("");
    match ty.as_str() {
        "TimeoutError" => "timeout",
        "MemoryError" => "memory_limit",
        "RecursionError" => "recursion_limit",
        "PermissionError" => "permission_denied",
        "SyntaxError" | "IndentationError" | "TabError" => "syntax_error",
        "NotImplementedError" => "unsupported_syntax",
        "RuntimeError" if msg.contains("host call limit exceeded") => "host_call_limit",
        "RuntimeError" if msg.contains("mutation call limit exceeded") => "mutation_call_limit",
        "RuntimeError" if msg.contains("suspension limit exceeded") => "suspension_limit",
        "TypeError" => "type_error",
        _ => "python_exception",
    }
}

fn build_error_outcome(
    err: Box<ReplStartError>,
    stdout_buf: String,
    calls: usize,
    mutations: usize,
    duration_ms: u64,
    limits: &TcCodeLimits,
    spill_dir: Option<&Path>,
) -> ExecuteOutcome {
    let exc = err.error;
    let kind = classify_exception_kind(&exc).to_owned();
    let exception_type = exc.exc_type().to_string();
    let message = exc
        .message()
        .map_or_else(|| exception_type.clone(), str::to_owned);
    let formatted_tb = Some(exc.to_string());

    finalize_error_with_stdout(
        ExecuteErrorInfo {
            kind,
            exception_type,
            message,
            traceback: formatted_tb,
        },
        stdout_buf,
        calls,
        mutations,
        duration_ms,
        limits,
        spill_dir,
    )
}

fn build_conversion_error_outcome(
    message: String,
    stdout_buf: String,
    calls: usize,
    mutations: usize,
    duration_ms: u64,
    limits: &TcCodeLimits,
    spill_dir: Option<&Path>,
) -> ExecuteOutcome {
    finalize_error_with_stdout(
        ExecuteErrorInfo {
            kind: "output_conversion_error".to_owned(),
            exception_type: "ValueError".to_owned(),
            message,
            traceback: None,
        },
        stdout_buf,
        calls,
        mutations,
        duration_ms,
        limits,
        spill_dir,
    )
}

fn finalize_error_with_stdout(
    error_info: ExecuteErrorInfo,
    stdout_buf: String,
    calls: usize,
    mutations: usize,
    duration_ms: u64,
    limits: &TcCodeLimits,
    spill_dir: Option<&Path>,
) -> ExecuteOutcome {
    let output_bytes = stdout_buf.len();
    let trunc_idx = stdout_buf
        .char_indices()
        .nth(limits.max_output_chars)
        .map(|(i, _)| i);
    let stdout_truncated = trunc_idx.is_some();
    let (spill_path, total_bytes, final_stdout) = if let Some(byte_idx) = trunc_idx {
        let capped_stdout = stdout_buf[..byte_idx].to_owned();
        let full = json!({
            "stdout": &stdout_buf,
            "calls": calls,
            "mutations": mutations,
            "error": &error_info,
        });
        let bytes = serde_json::to_vec(&full).ok();
        drop_json_iteratively(full);
        let (sp, tb) = bytes.map_or_else(
            || (None, Some(output_bytes)),
            |raw| {
                let len = raw.len();
                let path = if limits.allow_spill_files {
                    write_spill_bytes(&raw, spill_dir)
                } else {
                    None
                };
                (path, Some(len))
            },
        );
        (sp, tb, capped_stdout)
    } else {
        (None, None, stdout_buf)
    };

    ExecuteOutcome {
        status: ExecuteStatus::Error,
        value: None,
        stdout: final_stdout,
        calls,
        mutations,
        duration_ms,
        output_bytes,
        truncated: stdout_truncated.then_some(true),
        spill_path,
        total_bytes,
        error: Some(error_info),
    }
}

#[derive(Serialize)]
struct SpillEnvelope<'a> {
    value: &'a RawValue,
    stdout: &'a str,
    calls: usize,
    mutations: usize,
    duration_ms: u64,
    total_bytes: usize,
}

fn finalize_ok_outcome(
    value: Value,
    stdout_buf: String,
    calls: usize,
    mutations: usize,
    duration_ms: u64,
    limits: &TcCodeLimits,
    spill_dir: Option<&Path>,
) -> ExecuteOutcome {
    // Serialize `value` once to compact JSON, then drop the tree iteratively so we
    // never serialize it a second time or risk stack overflow on Drop.
    let value_json_str = serde_json::to_string(&value).unwrap_or_else(|_| "null".to_owned());
    let total_bytes = value_json_str.len().saturating_add(stdout_buf.len());
    let total_chars = value_json_str
        .chars()
        .count()
        .saturating_add(stdout_buf.chars().count());

    if total_chars <= limits.max_output_chars {
        return ExecuteOutcome {
            status: ExecuteStatus::Ok,
            value: Some(value),
            stdout: stdout_buf,
            calls,
            mutations,
            duration_ms,
            output_bytes: total_bytes,
            truncated: None,
            spill_path: None,
            total_bytes: None,
            error: None,
        };
    }

    drop_json_iteratively(value);

    let spill_path = if limits.allow_spill_files {
        RawValue::from_string(value_json_str.clone())
            .ok()
            .and_then(|raw_val| {
                let env = SpillEnvelope {
                    value: &raw_val,
                    stdout: &stdout_buf,
                    calls,
                    mutations,
                    duration_ms,
                    total_bytes,
                };
                serde_json::to_vec(&env).ok()
            })
            .and_then(|bytes| write_spill_bytes(&bytes, spill_dir))
    } else {
        None
    };

    let half_budget = (limits.max_output_chars / 2).max(128);
    let (stdout_preview, _) = cap_utf8_inline(&stdout_buf, half_budget);
    let remaining_budget = limits
        .max_output_chars
        .saturating_sub(stdout_preview.chars().count())
        .max(128);
    let (value_preview, _) = cap_utf8_inline(&value_json_str, remaining_budget);

    ExecuteOutcome {
        status: ExecuteStatus::Ok,
        value: Some(json!({
            "_truncated": true,
            "preview_json": value_preview,
            "spill_path": spill_path,
            "total_bytes": total_bytes,
        })),
        stdout: stdout_preview,
        calls,
        mutations,
        duration_ms,
        output_bytes: total_bytes,
        truncated: Some(true),
        spill_path,
        total_bytes: Some(total_bytes),
        error: None,
    }
}

fn cap_utf8_inline(text: &str, max_chars: usize) -> (String, bool) {
    let mut char_indices = text.char_indices();
    if let Some((byte_idx, _)) = char_indices.nth(max_chars) {
        (text[..byte_idx].to_owned(), true)
    } else {
        (text.to_owned(), false)
    }
}

fn write_spill_bytes(bytes: &[u8], custom_dir: Option<&Path>) -> Option<String> {
    let base_dir = custom_dir.map_or_else(std::env::temp_dir, Path::to_path_buf);
    let _ = std::fs::create_dir_all(&base_dir);
    let mut tmp = tempfile::Builder::new()
        .prefix("tc-code-spill-")
        .suffix(".json")
        .tempfile_in(&base_dir)
        .ok()?;
    tmp.write_all(bytes).ok()?;
    let _ = tmp.as_file().sync_data();
    let (_file, kept_path) = tmp.keep().ok()?;

    let evicted = {
        let mut ring = SPILL_RING.lock();
        let old = if ring.len() >= MAX_RETAINED_SPILL_FILES {
            ring.pop_front()
        } else {
            None
        };
        ring.push_back(kept_path.clone());
        old
    };
    if let Some(old_path) = evicted {
        let _ = std::fs::remove_file(old_path);
    }

    Some(kept_path.to_string_lossy().replace('\\', "/"))
}
