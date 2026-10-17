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
    let serial = hex::encode(crate::device::serial_bytes(&session.command(
        slot,
        1,
        0x3c,
        &[0],
        0x3e,
    )?)?);
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
    let journal_path = suffixed(prefix, ".journal.jsonl");
    let mut journal = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&journal_path)
        .context("batch journal must be a new file (choose a new backup prefix)")?;
    record(
        &mut journal,
        json!({"stage":"started","serial":serial,"slot":slot,"model":model.name(),"operations":operations}),
    )?;
    let mut completed = Vec::new();
    for (index, op) in operations.iter().enumerate() {
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
                Operation::Set { name, value } => Ok(serde_json::to_value(set_for_model(
                    session, slot, name, value, &backup, model,
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
                completed.push(value);
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
    Ok(
        json!({"serial":serial,"slot":slot,"completed":completed.len(),"results":completed,"journal":journal_path}),
    )
}
