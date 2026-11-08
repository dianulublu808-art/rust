use crate::modules::{Command, Context, Module};
use anyhow::{bail, Context as _, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use wasmi::{Config as WasmiConfig, Engine, Module as WasmModule, Store, StoreLimitsBuilder};

const MAX_MANIFEST_COMMANDS: usize = 64;
const MAX_WASM_BYTES: u64 = 8 * 1024 * 1024;
const MAX_INPUT_BYTES: usize = 32 * 1024;
const MAX_OUTPUT_BYTES: usize = 8 * 1024;
const MAX_PLUGIN_FUEL: u64 = 5_000_000;
const MAX_PLUGIN_MEMORY: usize = 16 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct PluginManifest {
    name: String,
    wasm: String,
    commands: Vec<PluginCommand>,
}

#[derive(Debug, Deserialize)]
struct PluginCommand {
    name: String,
    description: String,
    usage: String,
}

#[derive(Debug, Serialize)]
struct PluginInput<'a> {
    command: &'a str,
    args: &'a [String],
    args_raw: &'a str,
    message_text: &'a str,
    message_id: i32,
}

#[derive(Debug, Deserialize)]
struct PluginOutput {
    text: String,
}

struct WasmPlugin {
    name: String,
    commands: Vec<Command>,
    engine: Engine,
    module: WasmModule,
}

/// Load every `*.toml` plugin manifest from the plugin directory.
///
/// WebAssembly modules have no WASI or host imports. They can only transform the
/// supplied JSON into a text response; fuel and linear memory are capped per call.
pub fn load_plugins(plugin_dir: &str) -> Result<Vec<Arc<dyn Module>>> {
    let root = PathBuf::from(plugin_dir);
    fs::create_dir_all(&root)
        .with_context(|| format!("create plugin directory {}", root.display()))?;
    let root = root
        .canonicalize()
        .with_context(|| format!("resolve plugin directory {}", root.display()))?;

    let mut manifests = fs::read_dir(&root)
        .with_context(|| format!("read plugin directory {}", root.display()))?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("list plugin directory {}", root.display()))?;
    manifests.retain(|path| path.extension().is_some_and(|ext| ext == "toml"));
    manifests.sort();

    let engine = create_engine();
    let mut plugins: Vec<Arc<dyn Module>> = Vec::with_capacity(manifests.len());

    for manifest_path in manifests {
        let source = fs::read_to_string(&manifest_path)
            .with_context(|| format!("read plugin manifest {}", manifest_path.display()))?;
        let manifest: PluginManifest = toml::from_str(&source)
            .with_context(|| format!("parse plugin manifest {}", manifest_path.display()))?;
        validate_manifest(&manifest, &manifest_path)?;

        let wasm_relative = Path::new(&manifest.wasm);
        if wasm_relative.is_absolute() {
            bail!("plugin '{}' must use a relative wasm path", manifest.name);
        }
        let wasm_path = root.join(wasm_relative);
        let wasm_path = wasm_path
            .canonicalize()
            .with_context(|| format!("resolve WASM file for plugin '{}'", manifest.name))?;
        if !wasm_path.starts_with(&root) {
            bail!(
                "plugin '{}' WASM path escapes plugin directory",
                manifest.name
            );
        }

        let metadata = fs::metadata(&wasm_path)
            .with_context(|| format!("inspect plugin WASM {}", wasm_path.display()))?;
        if metadata.len() > MAX_WASM_BYTES {
            bail!(
                "plugin '{}' WASM exceeds the {} MiB size limit",
                manifest.name,
                MAX_WASM_BYTES / (1024 * 1024)
            );
        }
        let wasm_bytes = fs::read(&wasm_path)
            .with_context(|| format!("read plugin WASM {}", wasm_path.display()))?;
        let module = WasmModule::new(&engine, &mut &wasm_bytes[..])
            .with_context(|| format!("compile plugin '{}'", manifest.name))?;

        let commands: Vec<Command> = manifest
            .commands
            .into_iter()
            .map(|command| Command::new(command.name, command.description, command.usage))
            .collect();

        tracing::info!(
            plugin = %manifest.name,
            commands = commands.len(),
            "Loaded WASM plugin"
        );
        plugins.push(Arc::new(WasmPlugin {
            name: manifest.name,
            commands,
            engine: engine.clone(),
            module,
        }));
    }

    Ok(plugins)
}

fn validate_manifest(manifest: &PluginManifest, path: &Path) -> Result<()> {
    if !valid_name(&manifest.name) {
        bail!(
            "invalid plugin name '{}' in {}",
            manifest.name,
            path.display()
        );
    }
    if manifest.wasm.trim().is_empty() {
        bail!("plugin '{}' has an empty wasm path", manifest.name);
    }
    if manifest.commands.is_empty() || manifest.commands.len() > MAX_MANIFEST_COMMANDS {
        bail!(
            "plugin '{}' must define between 1 and {} commands",
            manifest.name,
            MAX_MANIFEST_COMMANDS
        );
    }

    let mut names = std::collections::HashSet::new();
    for command in &manifest.commands {
        if !valid_command_name(&command.name) {
            bail!(
                "invalid command name '{}' in plugin '{}'",
                command.name,
                manifest.name
            );
        }
        if !names.insert(command.name.to_ascii_lowercase()) {
            bail!(
                "duplicate command '{}' in plugin '{}'",
                command.name,
                manifest.name
            );
        }
        if command.description.trim().is_empty() || command.usage.trim().is_empty() {
            bail!(
                "command '{}' in plugin '{}' needs a description and usage",
                command.name,
                manifest.name
            );
        }
        if command.description.len() > 256 || command.usage.len() > 256 {
            bail!(
                "description or usage for '{}' in plugin '{}' is too long",
                command.name,
                manifest.name
            );
        }
    }
    Ok(())
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 48
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

fn valid_command_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn create_engine() -> Engine {
    let mut config = WasmiConfig::default();
    config.consume_fuel(true);
    Engine::new(&config)
}

#[async_trait]
impl Module for WasmPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn commands(&self) -> Vec<Command> {
        self.commands.clone()
    }

    async fn handle(&self, command: &str, ctx: Context) -> Result<()> {
        let payload = serde_json::to_vec(&PluginInput {
            command,
            args: &ctx.args,
            args_raw: &ctx.args_raw,
            message_text: ctx.message.text(),
            message_id: ctx.message.id(),
        })?;
        if payload.len() > MAX_INPUT_BYTES {
            bail!("plugin input exceeds {} bytes", MAX_INPUT_BYTES);
        }

        let engine = self.engine.clone();
        let module = self.module.clone();
        let output = tokio::task::spawn_blocking(move || invoke_wasm(&engine, &module, &payload))
            .await
            .context("WASM plugin worker failed")??;

        ctx.edit(output).await
    }
}

fn invoke_wasm(engine: &Engine, module: &WasmModule, input: &[u8]) -> Result<String> {
    let limits = StoreLimitsBuilder::new()
        .memory_size(MAX_PLUGIN_MEMORY)
        .instances(1)
        .memories(1)
        .tables(1)
        .table_elements(1024)
        .build();
    let mut store = Store::new(engine, limits);
    store.limiter(|limits| limits);
    store
        .set_fuel(MAX_PLUGIN_FUEL)
        .context("configure WASM fuel limit")?;

    // No host functions are registered: plugins cannot access files, network, or process APIs.
    let linker = wasmi::Linker::<wasmi::StoreLimits>::new(engine);
    let instance = linker
        .instantiate(&mut store, module)
        .context("instantiate WASM plugin (imports are not available)")?
        .start(&mut store)
        .context("start WASM plugin")?;

    let memory = instance
        .get_memory(&store, "memory")
        .context("WASM plugin must export memory as `memory`")?;
    let alloc = instance
        .get_typed_func::<i32, i32>(&store, "alloc")
        .context("WASM plugin must export alloc(i32) -> i32")?;
    let handle = instance
        .get_typed_func::<(i32, i32), i64>(&store, "handle")
        .context("WASM plugin must export handle(i32, i32) -> i64")?;

    let input_len = i32::try_from(input.len()).context("plugin input is too large")?;
    let input_ptr = alloc
        .call(&mut store, input_len)
        .context("WASM plugin input allocation failed")?;
    let input_ptr =
        usize::try_from(input_ptr).context("plugin returned a negative input pointer")?;
    memory
        .write(&mut store, input_ptr, input)
        .context("write plugin input to WASM memory")?;

    // ABI: handle returns (output_ptr << 32) | output_len.
    let packed = handle
        .call(&mut store, (input_ptr as i32, input_len))
        .context("WASM plugin handler trapped or exceeded its fuel limit")? as u64;
    let output_ptr = (packed >> 32) as usize;
    let output_len = (packed & 0xffff_ffff) as usize;
    if output_len == 0 || output_len > MAX_OUTPUT_BYTES {
        bail!("WASM plugin output length is outside the allowed range");
    }

    let mut bytes = vec![0; output_len];
    memory
        .read(&store, output_ptr, &mut bytes)
        .context("read plugin output from WASM memory")?;
    let output: PluginOutput =
        serde_json::from_slice(&bytes).context("plugin output must be JSON with a `text` field")?;
    if output.text.trim().is_empty() {
        bail!("WASM plugin returned empty text");
    }
    if output.text.len() > 3500 {
        bail!("WASM plugin response exceeds 3500 bytes");
    }
    Ok(output.text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn invokes_json_plugin_using_documented_abi() {
        let wat = r#"
            (module
                (memory (export "memory") 1 1)
                (data (i32.const 0) "{\"text\":\"ok\"}")
                (func (export "alloc") (param i32) (result i32)
                    i32.const 1024)
                (func (export "handle") (param i32 i32) (result i64)
                    i64.const 13))
        "#;
        let bytes = wat::parse_str(wat).expect("valid test WAT");
        let engine = create_engine();
        let module = WasmModule::new(&engine, &mut &bytes[..]).expect("valid test module");
        let output =
            invoke_wasm(&engine, &module, br#"{"command":"hello"}"#).expect("plugin executes");
        assert_eq!(output, "ok");
    }

    #[test]
    fn rejects_module_without_required_exports() {
        let bytes =
            wat::parse_str(r#"(module (memory (export "memory") 1))"#).expect("valid test WAT");
        let engine = create_engine();
        let module = WasmModule::new(&engine, &mut &bytes[..]).expect("valid test module");
        let error = invoke_wasm(&engine, &module, b"{}").expect_err("missing alloc must fail");
        assert!(error.to_string().contains("alloc"));
    }

    #[test]
    fn fuel_limit_stops_infinite_plugin_loop() {
        let wat = r#"
            (module
                (memory (export "memory") 1 1)
                (func (export "alloc") (param i32) (result i32)
                    i32.const 1024)
                (func (export "handle") (param i32 i32) (result i64)
                    (loop $forever
                        br $forever)
                    i64.const 0))
        "#;
        let bytes = wat::parse_str(wat).expect("valid test WAT");
        let engine = create_engine();
        let module = WasmModule::new(&engine, &mut &bytes[..]).expect("valid test module");
        let error = invoke_wasm(&engine, &module, b"{}")
            .expect_err("fuel limit must stop an infinite loop");
        assert!(error.to_string().contains("fuel limit"));
    }

    #[test]
    fn loads_manifest_and_registers_dynamic_commands() {
        let dir = tempfile::tempdir().expect("temporary plugin directory");
        let wasm = wat::parse_str(
            r#"(module
                (memory (export "memory") 1)
                (func (export "alloc") (param i32) (result i32) i32.const 0)
                (func (export "handle") (param i32 i32) (result i64) i64.const 0))"#,
        )
        .expect("valid test WAT");
        fs::write(dir.path().join("hello.wasm"), wasm).expect("write wasm file");
        let mut manifest =
            fs::File::create(dir.path().join("hello.toml")).expect("create manifest");
        manifest
            .write_all(
                br#"name = "hello"
wasm = "hello.wasm"

[[commands]]
name = "hello"
description = "Example plugin command"
usage = ".hello"
"#,
            )
            .expect("write manifest");

        let plugins = load_plugins(dir.path().to_str().unwrap()).expect("load manifest");
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name(), "hello");
        assert_eq!(plugins[0].commands()[0].name, "hello");
    }
}
