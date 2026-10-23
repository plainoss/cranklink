//! Ordered settings operations on an already opened and authenticated session.
//! Each write uses the ordinary durable-backup/readback implementation.
use super::*;
use serde::{Deserialize, Deserializer};
use std::path::PathBuf;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Operation {
    Read {
        name: Option<String>,
    },
    Set {
        name: String,
        #[serde(deserialize_with = "value_text")]
        value: String,
    },
    Restore {
        file: PathBuf,
        #[serde(skip)]
        parsed: Option<ParsedRestore>,
    },
}

fn value_text<'de, D: Deserializer<'de>>(deserializer: D) -> std::result::Result<String, D::Error> {
    match Value::deserialize(deserializer)? {
        Value::String(s) => Ok(s),
        Value::Number(n) => Ok(n.to_string()),
        _ => Err(serde::de::Error::custom(
            "setting value must be a string or number",
        )),
    }
}

/// Parse the entire plan before opening a transport. Restore inputs are relative
/// to the plan's directory; output prefixes retain normal CLI path semantics.
pub fn load(path: &Path) -> Result<Vec<Operation>> {
    let mut operations: Vec<Operation> = serde_json::from_slice(
        &crate::storage::read_bounded(path, 256 * 1024).context("read batch plan")?,
    )
    .context("batch plan must be a JSON array of read/set/restore operations")?;
    ensure!(
        !operations.is_empty() && operations.len() <= 128,
        "batch requires 1..=128 operations"
    );
    for op in &mut operations {
        if let Operation::Restore { file, .. } = op {
            if file.is_relative() {
                *file = path.parent().unwrap_or(Path::new(".")).join(&*file);
            }
        }
    }
    Ok(operations)
}

/// Parse and cache all restore files in a batch plan before opening a transport.
pub fn preload_restores(operations: &mut [Operation]) -> Result<()> {
    for (index, op) in operations.iter_mut().enumerate() {
        if let Operation::Restore { file, parsed } = op {
            if parsed.is_none() {
                let p = parse_restore_file(file).with_context(|| {
                    format!(
                        "batch operation {} preflight failed; no batch writes sent",
                        index + 1
                    )
                })?;
                *parsed = Some(p);
            }
        }
    }
    Ok(())
}

fn suffixed(prefix: &Path, suffix: &str) -> PathBuf {
    let mut path = prefix.as_os_str().to_owned();
    path.push(suffix);
    path.into()
}

#[derive(Clone, Copy)]
enum Step<'a> {
    Read,
    Region(u8),
    Limit(&'a str, i64),
    Other,
}

/// A destination write is refused while the current speed or power is outside
/// the new region, and a limit write while it is outside the current one. For a
/// region change listed first (as the web app does), run a later limit write
/// of the same change first when it fits both regions, so lowering a region
/// together with its speed, and undoing a raise, succeed. The region-dependent
/// part of the resulting sequence is checked before any write.
fn execution_order(
    session: &mut dyn UnitCommands,
    slot: u8,
    operations: &[Operation],
    restores: &[Option<ParsedRestore>],
    model: SettingsModel,
) -> Result<Vec<usize>> {
    let plan_order = (0..operations.len()).collect();
    let steps = operations
        .iter()
        .zip(restores)
        .map(|(op, restore)| {
            let (name, value) = match op {
                Operation::Read { .. } => return Ok(Step::Read),
                Operation::Set { name, value } => (name.as_str(), value.clone()),
                Operation::Restore { .. } => {
                    let parsed = restore.as_ref().expect("restore preflight verified");
                    (parsed.setting.as_str(), parsed.planned_value())
                }
            };
            let raw = || spec_for_model(name, model)?.raw_value(&value);
            Ok(if name == "destination" {
                Step::Region(raw()? as u8)
            } else if is_destination_limit(name) {
                Step::Limit(name, raw()?)
            } else {
                Step::Other
            })
        })
        .collect::<Result<Vec<_>>>()?;
    if !steps.iter().any(|step| matches!(step, Step::Region(_))) {
        return Ok(plan_order);
    }
    // An unset EP800 destination is unreadable; each write's own check decides.
    let Ok(mut region) = effective_destination(session, slot, model) else {
        return Ok(plan_order);
    };
    let mut limits: BTreeMap<String, i64> = destination_limits(session, slot, model)
        .context("batch preflight failed; no batch writes sent")?
        .into_iter()
        .collect();
    let mut order = Vec::with_capacity(steps.len());
    let mut moved = vec![false; steps.len()];
    for i in 0..steps.len() {
        if moved[i] {
            continue;
        }
        let mut run = vec![];
        let mut kept: Vec<(&str, i64)> = vec![];
        if let Step::Region(next) = steps[i] {
            // Never move a write past a read, another region change, or an
            // earlier write of the same setting that stays in place.
            for (j, step) in steps.iter().enumerate().skip(i + 1) {
                match *step {
                    Step::Read | Step::Region(_) => break,
                    Step::Limit(name, n)
                        if !kept.iter().any(|&(k, _)| k == name)
                            && validate_destination_limit(model, name, n, region).is_ok()
                            && validate_destination_limit(model, name, n, next).is_ok() =>
                    {
                        run.push(j);
                        moved[j] = true;
                    }
                    Step::Limit(name, n) => kept.push((name, n)),
                    Step::Other => {}
                }
            }
        }
        run.push(i);
        for k in run {
            let checked = match steps[k] {
                Step::Region(next) => {
                    let from = std::mem::replace(&mut region, next);
                    limits.iter().try_for_each(|(name, &n)| {
                        validate_destination_limit(model, name, n, next).with_context(|| {
                            // Neither order works when the planned value does
                            // not fit the current destination either.
                            let stuck = kept
                                .iter()
                                .find(|&&(k, _)| k == name.as_str())
                                .and_then(|&(_, p)| {
                                    validate_destination_limit(model, name, p, from).err()
                                });
                            match stuck {
                                Some(e) => format!(
                                    "change {name} before destination {next}; the planned {name} is outside destination {from} too ({e}), so first save one both destinations permit"
                                ),
                                None => format!("change {name} before destination {next}"),
                            }
                        })
                    })
                }
                Step::Limit(name, n) => {
                    limits.insert(name.into(), n);
                    validate_destination_limit(model, name, n, region)
                }
                Step::Read | Step::Other => Ok(()),
            };
            checked.with_context(|| {
                format!(
                    "batch operation {} preflight failed; no batch writes sent",
                    k + 1
                )
            })?;
            order.push(k);
        }
    }
    Ok(order)
}

fn record(file: &mut crate::storage::File, mut value: Value) -> Result<()> {
    value["time_ms"] = json!(crate::platform::now_ms());
    serde_json::to_writer(&mut *file, &value)?;
    file.write_all(b"\n")?;
    file.sync_all().context("persist batch journal")
}

pub fn run(
    session: &mut dyn UnitCommands,
    slot: u8,
    operations: &[Operation],
    prefix: &Path,
    model: SettingsModel,
) -> Result<Value> {
    run_bound(session, slot, operations, prefix, model, None)
}

/// [`run`] on the unit the session verified: the batch's serial must equal
/// `expected_serial`, and each set re-checks it before its backup and SET.
pub fn run_bound(
    session: &mut dyn UnitCommands,
    slot: u8,
    operations: &[Operation],
    prefix: &Path,
    model: SettingsModel,
    expected_serial: Option<&str>,
) -> Result<Value> {
    ensure!(
        !operations.is_empty() && operations.len() <= 128,
        "batch requires 1..=128 operations"
    );
    ensure!(
        !prefix.as_os_str().is_empty(),
        "backup prefix must not be empty"
    );
    // Check every static name/value and restore input before the first write.
    // Model-dependent combinations (e.g. speed after region) use live reads at
    // each step, so later operations see changes made earlier in this session.
    let serial = checked_serial(session, slot, expected_serial)?;
    let mut resolved_restores: Vec<Option<ParsedRestore>> = Vec::with_capacity(operations.len());
    for (index, op) in operations.iter().enumerate() {
        let valid = (|| -> Result<Option<ParsedRestore>> {
            match op {
                Operation::Read { name: Some(name) } => {
                    spec_for_model(name, model)?;
                    Ok(None)
                }
                Operation::Read { name: None } => Ok(None),
                Operation::Set { name, value } => {
                    validate_input(&spec_for_model(name, model)?, value)?;
                    Ok(None)
                }
                Operation::Restore { file, parsed } => {
                    let parsed = match parsed {
                        Some(p) => p.clone(),
                        None => parse_restore_file(file)?,
                    };
                    parsed.validate_for_model(model, &serial)?;
                    Ok(Some(parsed))
                }
            }
        })();
        let parsed = valid.with_context(|| {
            format!(
                "batch operation {} preflight failed; no batch writes sent",
                index + 1
            )
        })?;
        resolved_restores.push(parsed);
    }
    let order = execution_order(session, slot, operations, &resolved_restores, model)?;
    let journal_path = suffixed(prefix, ".journal.jsonl");
    let mut journal = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&journal_path)
        .context("batch journal must be a new file (choose a new backup prefix)")?;
    record(
        &mut journal,
        json!({"stage":"started","serial":serial,"slot":slot,"model":model.name(),"operations":operations,
            "order":order.iter().map(|index| index + 1).collect::<Vec<_>>()}),
    )?;
    crate::storage::sync_parent(&journal_path).context("persist batch journal")?;
    let mut completed = Vec::new();
    for index in order {
        let op = &operations[index];
        let backup = suffixed(prefix, &format!(".{:03}.backup.json", index + 1));
        record(
            &mut journal,
            json!({"stage":"attempt","operation":index + 1,"backup":backup}),
        )?;
        let result = (|| -> Result<Value> {
            match op {
                Operation::Read { name: Some(name) } => Ok(serde_json::to_value(read_for_model(
                    session, slot, name, model,
                )?)?),
                Operation::Read { name: None } => {
                    let snapshot = read_all_for_model(session, slot, model);
                    ensure!(
                        snapshot.unavailable.is_empty(),
                        "batch full read incomplete: {:?}",
                        snapshot.unavailable
                    );
                    let mut value = serde_json::to_value(snapshot)?;
                    value["capabilities"] = capabilities_for_model(model);
                    Ok(value)
                }
                // Each write re-reads the serial for its backup; a unit that
                // took over the slot mid-batch gets no backup and no SET.
                Operation::Set { name, value } => Ok(serde_json::to_value(set_for_model_bound(
                    session,
                    slot,
                    name,
                    value,
                    &backup,
                    model,
                    Some(&serial),
                )?)?),
                Operation::Restore { .. } => {
                    let parsed = resolved_restores[index]
                        .as_ref()
                        .expect("restore preflight verified");
                    Ok(serde_json::to_value(restore_parsed_for_model(
                        session, slot, parsed, &backup, model,
                    )?)?)
                }
            }
        })();
        match result {
            Ok(value) => {
                record(
                    &mut journal,
                    json!({"stage":"completed","operation":index + 1,"result":value}),
                )
                .with_context(|| {
                    format!(
                        "operation {} completed but journal failed; stop and inspect {}",
                        index + 1,
                        backup.display()
                    )
                })?;
                completed.push((index, value));
            }
            Err(error) => {
                let failure = json!({"stage":"failed","operation":index + 1,"completed":completed.len(),"error":format!("{error:#}"),"backup":backup});
                let saved = record(&mut journal, failure);
                let detail = saved
                    .err()
                    .map(|e| format!("; failed to persist error: {e:#}"))
                    .unwrap_or_default();
                anyhow::bail!("batch stopped at operation {}; {} completed operations remain applied; inspect {}{}: {error:#}", index + 1, completed.len(), journal_path.display(), detail);
            }
        }
    }
    record(
        &mut journal,
        json!({"stage":"operations-completed","completed":completed.len(),"cleanup":"performed by caller after batch"}),
    )?;
    // Results keep plan order even when a region change ran a limit write first.
    completed.sort_by_key(|(index, _)| *index);
    let results: Vec<Value> = completed.into_iter().map(|(_, value)| value).collect();
    Ok(
        json!({"serial":serial,"slot":slot,"completed":results.len(),"results":results,"journal":journal_path}),
    )
}
