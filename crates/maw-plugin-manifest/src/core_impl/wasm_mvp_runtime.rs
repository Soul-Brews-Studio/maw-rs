fn render_version_output(plugin: &LoadedPlugin) -> String {
    let manifest = &plugin.manifest;
    format!(
        "{} v{} ({}, weight:{})\n  {}\n  surfaces: {}\n  dir: {}",
        manifest.name,
        manifest.version,
        plugin.kind.as_str(),
        manifest.weight.unwrap_or(50),
        manifest.description.as_deref().unwrap_or_default(),
        version_surfaces(plugin),
        plugin.dir.display()
    )
}

fn render_help_output(plugin: &LoadedPlugin) -> String {
    let manifest = &plugin.manifest;
    let effective_cmd = effective_cli_command(plugin);
    let mut lines = Vec::new();
    lines.push(format!("{} v{}", manifest.name, manifest.version));
    if let Some(description) = &manifest.description {
        lines.push(format!("  {description}"));
    }
    lines.push(String::new());
    if let Some(help) = manifest.cli.as_ref().and_then(|cli| cli.help.as_ref()) {
        lines.push(format!("  usage: {help}"));
    } else if let Some(command) = &effective_cmd {
        lines.push(format!("  usage: maw {command}"));
    }
    if let Some(aliases) = manifest.cli.as_ref().and_then(|cli| cli.aliases.as_ref()) {
        if !aliases.is_empty() {
            lines.push(format!("  aliases: {}", aliases.join(", ")));
        }
    }
    if let Some(flags) = manifest.cli.as_ref().and_then(|cli| cli.flags.as_ref()) {
        lines.push("  flags:".to_owned());
        for (name, kind) in flags {
            lines.push(format!("    {name:<20} {}", kind.as_str()));
        }
    }
    lines.push(String::new());
    lines.push("  surfaces:".to_owned());
    if let Some(command) = effective_cmd {
        lines.push(format!("    cli: maw {command}"));
    }
    if let Some(api) = &manifest.api {
        lines.push(format!(
            "    api: {} {}",
            api.methods
                .iter()
                .map(|method| method.as_str())
                .collect::<Vec<_>>()
                .join("/"),
            api.path
        ));
    }
    if manifest
        .transport
        .as_ref()
        .and_then(|transport| transport.peer)
        .unwrap_or(false)
    {
        lines.push(format!("    peer: maw hey plugin:{}", manifest.name));
    }
    if let Some(hooks) = help_hook_keys(manifest.hooks.as_ref()) {
        lines.push(format!("    hooks: {}", hooks.join(", ")));
    }
    lines.push(format!("\n  dir: {}", plugin.dir.display()));
    lines.join("\n")
}

fn version_surfaces(plugin: &LoadedPlugin) -> String {
    let manifest = &plugin.manifest;
    let mut surfaces = Vec::new();
    if let Some(command) = effective_cli_command(plugin) {
        surfaces.push(format!("cli:{command}"));
    }
    if let Some(api) = &manifest.api {
        surfaces.push(format!("api:{}", api.path));
    }
    if manifest.hooks.is_some() {
        surfaces.push("hooks".to_owned());
    }
    if manifest
        .transport
        .as_ref()
        .and_then(|transport| transport.peer)
        .unwrap_or(false)
    {
        surfaces.push("peer".to_owned());
    }
    surfaces.join(", ")
}

fn effective_cli_command(plugin: &LoadedPlugin) -> Option<String> {
    plugin.manifest.cli.as_ref().map_or_else(
        || {
            let dispatchable = match plugin.kind {
                LoadedPluginKind::Ts => plugin.entry_path.is_some(),
                LoadedPluginKind::Wasm => !plugin.wasm_path.as_os_str().is_empty(),
            };
            dispatchable.then(|| plugin.manifest.name.clone())
        },
        |cli| Some(cli.command.clone()),
    )
}

fn help_hook_keys(hooks: Option<&PluginHooks>) -> Option<Vec<&'static str>> {
    let hooks = hooks?;
    let mut keys = Vec::new();
    if hooks.gate.is_some() {
        keys.push("gate");
    }
    if hooks.filter.is_some() {
        keys.push("filter");
    }
    if hooks.on.is_some() {
        keys.push("on");
    }
    if hooks.late.is_some() {
        keys.push("late");
    }
    if hooks.wake.is_some() {
        keys.push("wake");
    }
    if hooks.sleep.is_some() {
        keys.push("sleep");
    }
    if hooks.serve.is_some() {
        keys.push("serve");
    }
    Some(keys)
}

fn invoke_wasm_mvp(ctx: &InvokeContext, wasm_bytes: &[u8]) -> InvokeResult {
    let module = match MvpWasmModule::parse(wasm_bytes) {
        Ok(module) => module,
        Err(error) => return InvokeResult::error(format!("wasm compile error: {error}")),
    };
    if !module.exports_handle || !module.exports_memory {
        return InvokeResult::error("wasm missing required handle+memory exports");
    }
    if module.has_imports {
        return InvokeResult::error("wasm instantiation failed: unresolved imports");
    }

    let mut memory = vec![0_u8; 65_536];
    for segment in &module.data_segments {
        let start = segment.offset;
        if start < memory.len() {
            let len = segment.bytes.len().min(memory.len() - start);
            memory[start..start + len].copy_from_slice(&segment.bytes[..len]);
        }
    }

    let ctx_json = serde_json::json!({
        "source": ctx.source.as_str(),
        "args": &ctx.args,
    })
    .to_string();
    write_linear_memory(&mut memory, 0, ctx_json.as_bytes());

    read_wasm_result_from_memory(&memory, module.handle_result)
}

#[derive(Debug, Default)]
struct MvpWasmModule {
    has_imports: bool,
    exports_memory: bool,
    exports_handle: bool,
    handle_result: i32,
    data_segments: Vec<MvpDataSegment>,
}

#[derive(Debug)]
struct MvpDataSegment {
    offset: usize,
    bytes: Vec<u8>,
}

impl MvpWasmModule {
    fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.get(..8) != Some(&[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]) {
            return Err("failed to parse WebAssembly module".to_owned());
        }
        let mut cursor = WasmCursor::new(&bytes[8..]);
        let mut module = Self::default();
        let mut imported_func_count = 0_u32;
        let mut defined_func_count = 0_u32;
        let mut exported_handle_index = None;
        while !cursor.is_empty() {
            let id = cursor.read_u8()?;
            if id == 0 || id > 12 {
                return Err("failed to parse WebAssembly module".to_owned());
            }
            let section_len = cursor.read_leb_usize()?;
            let section = cursor.read_bytes(section_len)?;
            match id {
                2 => {
                    let count = count_section_items(section)?;
                    module.has_imports = count > 0;
                    imported_func_count = count_imported_functions(section)?;
                }
                3 => defined_func_count = count_section_items(section)?,
                7 => exported_handle_index = parse_exports(section, &mut module)?,
                10 => parse_optional_code_section(
                    section,
                    exported_handle_index,
                    imported_func_count,
                    defined_func_count,
                    &mut module,
                )?,
                11 => module.data_segments = parse_data_segments(section)?,
                _ => {}
            }
        }
        Ok(module)
    }
}

fn parse_optional_code_section(
    section: &[u8],
    exported_handle_index: Option<u32>,
    imported_func_count: u32,
    defined_func_count: u32,
    module: &mut MvpWasmModule,
) -> Result<(), String> {
    let Some(index) = exported_handle_index else {
        return Ok(());
    };
    let handle_result = parse_handle_result(
        section,
        index,
        imported_func_count,
        defined_func_count,
    )?;
    module.handle_result = handle_result;
    Ok(())
}

struct WasmCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> WasmCursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn is_empty(&self) -> bool {
        self.offset >= self.bytes.len()
    }

    fn read_u8(&mut self) -> Result<u8, String> {
        let Some(byte) = self.bytes.get(self.offset).copied() else {
            return Err("failed to parse WebAssembly module".to_owned());
        };
        self.offset += 1;
        Ok(byte)
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], String> {
        let Some(bytes) = self.bytes.get(self.offset..self.offset + len) else {
            return Err("failed to parse WebAssembly module".to_owned());
        };
        self.offset += len;
        Ok(bytes)
    }

    fn read_leb_u32(&mut self) -> Result<u32, String> {
        let mut result = 0_u32;
        let mut shift = 0;
        loop {
            let byte = self.read_u8()?;
            result |= u32::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(result);
            }
            shift += 7;
            if shift >= 32 {
                return Err("failed to parse WebAssembly module".to_owned());
            }
        }
    }

    fn read_leb_usize(&mut self) -> Result<usize, String> {
        usize::try_from(self.read_leb_u32()?)
            .map_err(|_| "failed to parse WebAssembly module".to_owned())
    }
}

fn count_section_items(section: &[u8]) -> Result<u32, String> {
    WasmCursor::new(section).read_leb_u32()
}

fn count_imported_functions(section: &[u8]) -> Result<u32, String> {
    let mut cursor = WasmCursor::new(section);
    let count = cursor.read_leb_u32()?;
    let mut funcs = 0_u32;
    for _ in 0..count {
        skip_name(&mut cursor)?;
        skip_name(&mut cursor)?;
        let kind = cursor.read_u8()?;
        match kind {
            0x00 => {
                let _type_index = cursor.read_leb_u32()?;
                funcs += 1;
            }
            0x01 | 0x02 => skip_limits(&mut cursor)?,
            0x03 => {
                let _content_type = cursor.read_u8()?;
                let _mutable = cursor.read_u8()?;
            }
            _ => return Err("failed to parse WebAssembly module".to_owned()),
        }
    }
    Ok(funcs)
}

fn parse_exports(section: &[u8], module: &mut MvpWasmModule) -> Result<Option<u32>, String> {
    let mut cursor = WasmCursor::new(section);
    let count = cursor.read_leb_u32()?;
    let mut handle = None;
    for _ in 0..count {
        let name = read_name(&mut cursor)?;
        let kind = cursor.read_u8()?;
        let index = cursor.read_leb_u32()?;
        if name == "memory" && kind == 0x02 {
            module.exports_memory = true;
        }
        if name == "handle" && kind == 0x00 {
            module.exports_handle = true;
            handle = Some(index);
        }
    }
    Ok(handle)
}

fn parse_handle_result(
    section: &[u8],
    handle_index: u32,
    imported_func_count: u32,
    defined_func_count: u32,
) -> Result<i32, String> {
    if handle_index < imported_func_count {
        return Err("failed to parse WebAssembly module".to_owned());
    }
    let body_index = handle_index - imported_func_count;
    if body_index >= defined_func_count {
        return Err("failed to parse WebAssembly module".to_owned());
    }
    let mut cursor = WasmCursor::new(section);
    let count = cursor.read_leb_u32()?;
    for index in 0..count {
        let body_len = cursor.read_leb_usize()?;
        let body = cursor.read_bytes(body_len)?;
        if index == body_index {
            return parse_const_i32_body(body);
        }
    }
    Err("failed to parse WebAssembly module".to_owned())
}

fn parse_const_i32_body(body: &[u8]) -> Result<i32, String> {
    let mut cursor = WasmCursor::new(body);
    let local_groups = cursor.read_leb_u32()?;
    for _ in 0..local_groups {
        let _count = cursor.read_leb_u32()?;
        let _type = cursor.read_u8()?;
    }
    if cursor.read_u8()? != 0x41 {
        return Err("failed to parse WebAssembly module".to_owned());
    }
    let value = read_signed_leb_i32(&mut cursor)?;
    if cursor.read_u8()? != 0x0b {
        return Err("failed to parse WebAssembly module".to_owned());
    }
    Ok(value)
}

fn parse_data_segments(section: &[u8]) -> Result<Vec<MvpDataSegment>, String> {
    let mut cursor = WasmCursor::new(section);
    let count = cursor.read_leb_u32()?;
    let mut segments = Vec::new();
    for _ in 0..count {
        let flag = cursor.read_leb_u32()?;
        if flag != 0 {
            return Err("failed to parse WebAssembly module".to_owned());
        }
        if cursor.read_u8()? != 0x41 {
            return Err("failed to parse WebAssembly module".to_owned());
        }
        let offset = usize::try_from(read_signed_leb_i32(&mut cursor)?)
            .map_err(|_| "failed to parse WebAssembly module".to_owned())?;
        if cursor.read_u8()? != 0x0b {
            return Err("failed to parse WebAssembly module".to_owned());
        }
        let len = cursor.read_leb_usize()?;
        segments.push(MvpDataSegment {
            offset,
            bytes: cursor.read_bytes(len)?.to_vec(),
        });
    }
    Ok(segments)
}

fn read_name(cursor: &mut WasmCursor<'_>) -> Result<String, String> {
    let len = cursor.read_leb_usize()?;
    let bytes = cursor.read_bytes(len)?;
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| "failed to parse WebAssembly module".to_owned())
}

fn skip_name(cursor: &mut WasmCursor<'_>) -> Result<(), String> {
    let len = cursor.read_leb_usize()?;
    let _bytes = cursor.read_bytes(len)?;
    Ok(())
}

fn skip_limits(cursor: &mut WasmCursor<'_>) -> Result<(), String> {
    let flags = cursor.read_u8()?;
    let _min = cursor.read_leb_u32()?;
    if flags & 0x01 != 0 {
        let _max = cursor.read_leb_u32()?;
    }
    Ok(())
}

fn read_signed_leb_i32(cursor: &mut WasmCursor<'_>) -> Result<i32, String> {
    let mut result = 0_i32;
    let mut shift = 0;
    let mut byte;
    loop {
        byte = cursor.read_u8()?;
        result |= i32::from(byte & 0x7f) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            break;
        }
        if shift >= 32 {
            return Err("failed to parse WebAssembly module".to_owned());
        }
    }
    if shift < 32 && byte & 0x40 != 0 {
        result |= !0_i32 << shift;
    }
    Ok(result)
}

fn write_linear_memory(memory: &mut [u8], ptr: usize, bytes: &[u8]) {
    if ptr >= memory.len() {
        return;
    }
    let len = bytes.len().min(memory.len() - ptr);
    memory[ptr..ptr + len].copy_from_slice(&bytes[..len]);
}

fn read_wasm_result_from_memory(memory: &[u8], result_ptr: i32) -> InvokeResult {
    if result_ptr <= 0 {
        return InvokeResult::ok();
    }
    #[allow(clippy::cast_sign_loss)]
    let start = result_ptr as usize;
    if start >= memory.len() {
        return InvokeResult::ok();
    }

    if let Some(output) = read_length_prefixed_wasm_output(memory, start) {
        return InvokeResult::output(output);
    }
    let end = memory[start..]
        .iter()
        .position(|byte| *byte == 0)
        .map_or(memory.len(), |offset| start + offset);
    let output = String::from_utf8_lossy(&memory[start..end]).into_owned();
    if output.is_empty() {
        InvokeResult::ok()
    } else {
        InvokeResult::output(output)
    }
}

fn read_length_prefixed_wasm_output(data: &[u8], start: usize) -> Option<String> {
    let len_bytes = data.get(start..start + 4)?;
    let len = u32::from_le_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]]) as usize;
    if len == 0 || len >= 1_000_000 {
        return None;
    }
    let payload_start = start + 4;
    let payload = data.get(payload_start..payload_start + len)?;
    Some(String::from_utf8_lossy(payload).into_owned())
}

fn cached_discover_plugins() -> Option<Vec<LoadedPlugin>> {
    discover_cache().lock().ok().and_then(|cache| cache.clone())
}

#[cfg(test)]
mod part04_coverage_tests {
    use super::*;

    #[test]
    fn wasm_cursor_reports_short_reads_and_overlong_leb_lengths() {
        let mut empty = WasmCursor::new(&[]);
        assert_eq!(
            empty.read_u8().expect_err("empty read_u8"),
            "failed to parse WebAssembly module"
        );

        let mut cursor = WasmCursor::new(&[0x7f, 0x02, b'a']);
        assert_eq!(cursor.read_u8().expect("byte"), 0x7f);
        assert_eq!(cursor.read_leb_usize().expect("usize leb"), 2);
        assert_eq!(
            cursor.read_bytes(2).expect_err("short payload"),
            "failed to parse WebAssembly module"
        );

        let mut overlong = WasmCursor::new(&[0x80, 0x80, 0x80, 0x80, 0x80]);
        assert_eq!(
            overlong.read_leb_usize().expect_err("overlong leb"),
            "failed to parse WebAssembly module"
        );
    }

    #[test]
    fn parse_data_segments_rejects_negative_offsets_and_accepts_payload() {
        assert_eq!(
            parse_data_segments(&[0x01, 0x00, 0x41, 0x7f, 0x0b, 0x00])
                .expect_err("negative offset"),
            "failed to parse WebAssembly module"
        );

        let segments =
            parse_data_segments(&[0x01, 0x00, 0x41, 0x02, 0x0b, 0x03, b'a', b'b', b'c'])
                .expect("data segment");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].offset, 2);
        assert_eq!(segments[0].bytes, b"abc");
    }

    #[test]
    fn length_prefixed_wasm_output_handles_length_edges() {
        assert_eq!(read_length_prefixed_wasm_output(&[1, 0, 0], 0), None);
        assert_eq!(read_length_prefixed_wasm_output(&[0, 0, 0, 0], 0), None);
        assert_eq!(
            read_length_prefixed_wasm_output(&1_000_000_u32.to_le_bytes(), 0),
            None
        );
        assert_eq!(
            read_length_prefixed_wasm_output(&[3, 0, 0, 0, b'a'], 0),
            None
        );

        let data = [0, 9, 9, 2, 0, 0, 0, b'o', b'k'];
        assert_eq!(
            read_length_prefixed_wasm_output(&data, 3),
            Some("ok".to_owned())
        );
    }
}
